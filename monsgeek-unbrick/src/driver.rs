//! Self-signed WinUSB driver installation for AT32F405 ROM DFU device (2E3C:DF11).
//!
//! Replicates the Zadig flow: create ephemeral self-signed cert, sign a catalog
//! referencing the INF, install the driver package, then remove the cert.

use anyhow::{bail, Result};
use std::mem;
use std::path::Path;
use std::ptr;

use windows_sys::core::GUID;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    SetupCopyOEMInfW, UpdateDriverForPlugAndPlayDevicesW, INSTALLFLAG_FORCE, SPOST_PATH,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, BOOL, FALSE, GENERIC_ACCESS_RIGHTS, HANDLE,
    INVALID_HANDLE_VALUE, SYSTEMTIME, TRUE,
};
use windows_sys::Win32::Security::Cryptography::Catalog::{
    CryptCATAdminAcquireContext, CryptCATAdminCalcHashFromFileHandle, CryptCATAdminReleaseContext,
    CryptCATClose, CryptCATOpen, CryptCATPersistStore, CryptCATPutAttrInfo,
    CryptCATPutCatAttrInfo, CryptCATPutMemberInfo, CRYPTCAT_ATTR_AUTHENTICATED,
    CRYPTCAT_ATTR_DATAASCII, CRYPTCAT_ATTR_NAMEASCII, CRYPTCAT_OPEN_CREATENEW,
    CRYPTCAT_VERSION_1,
};
use windows_sys::Win32::Security::Cryptography::{
    CertAddCertificateContextToStore, CertCloseStore, CertCreateSelfSignCertificate,
    CertDeleteCertificateFromStore, CertFindCertificateInStore, CertFreeCertificateContext,
    CertOpenStore, CertStrToNameW, CryptAcquireContextW, CryptEncodeObjectEx,
    SignerFreeSignerContext, SignerSignEx, AT_SIGNATURE, CALG_SHA_256, CERT_CONTEXT,
    CERT_EXTENSION, CERT_EXTENSIONS, CERT_FIND_EXISTING, CERT_STORE_ADD_REPLACE_EXISTING,
    CERT_STORE_PROV_SYSTEM_W, CERT_SYSTEM_STORE_LOCAL_MACHINE, CERT_X500_NAME_STR,
    CRYPT_ALGORITHM_IDENTIFIER, CRYPT_DELETE_KEYSET, CRYPT_ENCODE_ALLOC_FLAG,
    CRYPT_INTEGER_BLOB, CRYPT_KEY_PROV_INFO, CRYPT_MACHINE_KEYSET, CTL_USAGE,
    PKCS_7_ASN_ENCODING, PROV_RSA_FULL, SIGNER_CERT, SIGNER_CERT_POLICY_CHAIN,
    SIGNER_CERT_STORE, SIGNER_CERT_STORE_INFO, SIGNER_CONTEXT, SIGNER_FILE_INFO,
    SIGNER_NO_ATTR, SIGNER_SIGNATURE_INFO, SIGNER_SUBJECT_FILE, SIGNER_SUBJECT_INFO,
    X509_ASN_ENCODING,
};
use windows_sys::Win32::Security::Cryptography::Sip::SIP_INDIRECT_DATA;
use windows_sys::Win32::Security::Cryptography::CRYPT_ATTRIBUTE_TYPE_VALUE;
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, OPEN_EXISTING};

const GENERIC_READ: GENERIC_ACCESS_RIGHTS = 0x80000000;

// WinUSB INF for AT32F405 ROM DFU device
const INF_CONTENT: &str = r#"[Version]
Signature   = "$Windows NT$"
Class       = USBDevice
ClassGuid   = {88BAE032-5A81-49F0-BC3D-A4FF138216D6}
Provider    = %ProviderName%
CatalogFile = monsgeek-dfu.cat
DriverVer   = 01/01/2025,1.0.0.0

[Manufacturer]
%MfgName% = DeviceList,NTamd64

[DeviceList.NTamd64]
%DeviceName% = USB_Install, USB\VID_2E3C&PID_DF11

[USB_Install]
Include = winusb.inf
Needs   = WINUSB.NT

[USB_Install.Services]
Include = winusb.inf
Needs   = WINUSB.NT.Services

[USB_Install.HW]
AddReg = Dev_AddReg

[Dev_AddReg]
HKR,,DeviceInterfaceGUIDs,0x10000,"{D1975C4A-3FCF-4B96-B23E-5B8E3B09C8F8}"

[Strings]
ProviderName = "MonsGeek"
MfgName      = "Artery Technology"
DeviceName   = "AT32F405 DFU (WinUSB)"
"#;

const CONTAINER_NAME: &str = "MonsGeek Recovery";
const SUBJECT_NAME: &str = "CN=MonsGeek Recovery";

// OID byte strings (null-terminated ASCII)
const OID_CODE_SIGNING: &[u8] = b"1.3.6.1.5.5.7.3.3\0";
const OID_ENHANCED_KEY_USAGE: &[u8] = b"2.5.29.37\0";
const OID_SHA256_RSA: &[u8] = b"1.2.840.113549.1.1.11\0";

// CryptEncodeObjectEx structure type (integer-as-pointer constant)
const X509_ENHANCED_KEY_USAGE_TYPE: *const u8 = 36 as *const u8;

// GUID for INF subject type in catalog (from libwdi/pki.c)
const INF_SUBJECT_GUID: GUID = GUID {
    data1: 0xDE351A42,
    data2: 0x8E59,
    data3: 0x11D0,
    data4: [0x8C, 0x47, 0x00, 0xC0, 0x4F, 0xC2, 0x95, 0xEE],
};

// OID for CAB/INF SIP indirect data
const SPC_CAB_DATA_OBJID: &[u8] = b"1.3.6.1.4.1.311.2.1.25\0";
// OID for SHA1 digest algorithm
const OID_OIWSEC_SHA1: &[u8] = b"1.3.14.3.2.26\0";


/// Null-terminated UTF-16 string.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error_code() -> u32 {
    unsafe { GetLastError() }
}

fn win_err(context: &str) -> anyhow::Error {
    anyhow::anyhow!("{}: Win32 error 0x{:08X}", context, last_error_code())
}

/// Log a message to both stdout and the persistent log file.
fn log(msg: &str) {
    println!("{msg}");
    let _ = crate::append_log(msg);
}

/// Install a WinUSB driver for the AT32F405 ROM DFU device.
///
/// Creates an ephemeral self-signed certificate, signs a catalog that references
/// the INF, installs the driver package, then removes the certificate.
pub fn install_winusb_driver() -> Result<()> {
    log("Installing WinUSB driver for AT32F405 DFU device...");
    unsafe { install_inner() }
}

unsafe fn install_inner() -> Result<()> {
    // Clean up any leftover key container from a previous run
    let _ = delete_key_container();

    // 1. Write INF to temp dir
    let temp_dir = std::env::temp_dir().join("monsgeek-dfu-driver");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir)?;

    let inf_path = temp_dir.join("monsgeek-dfu.inf");
    let cat_path = temp_dir.join("monsgeek-dfu.cat");
    std::fs::write(&inf_path, INF_CONTENT)?;

    // 2. Create self-signed certificate
    log("  Creating self-signed certificate...");
    let cert = create_self_signed_cert()?;

    // Steps 3-6, with cleanup guaranteed
    let result = (|| -> Result<()> {
        // 3. Install cert to stores
        log("  Installing certificate to trusted stores...");
        install_cert_to_store(cert, "Root")?;
        install_cert_to_store(cert, "TrustedPublisher")?;

        // 4. Create catalog
        log("  Creating catalog file...");
        create_catalog(&inf_path, &cat_path)?;
        log("  Catalog created OK.");

        // 5. Sign catalog
        log("  Signing catalog...");
        sign_catalog(&cat_path, cert)?;
        log("  Catalog signed OK.");

        // 6. Install driver
        log("  Installing driver...");
        install_driver(&inf_path)?;

        Ok(())
    })();

    // 7. Cleanup (always, even on error)
    log("  Cleaning up certificates...");
    let _ = remove_cert_from_store(cert, "Root");
    let _ = remove_cert_from_store(cert, "TrustedPublisher");
    let _ = delete_key_container();
    CertFreeCertificateContext(cert);
    let _ = std::fs::remove_dir_all(&temp_dir);

    result
}

/// Create a self-signed code signing certificate (RSA 2048, SHA256, 10-year validity).
unsafe fn create_self_signed_cert() -> Result<*const CERT_CONTEXT> {
    let container = wide(CONTAINER_NAME);
    let subject = wide(SUBJECT_NAME);

    // Key provider info — CertCreateSelfSignCertificate will auto-create the key
    let key_prov_info = CRYPT_KEY_PROV_INFO {
        pwszContainerName: container.as_ptr() as *mut u16,
        pwszProvName: ptr::null_mut(),
        dwProvType: PROV_RSA_FULL,
        dwFlags: CRYPT_MACHINE_KEYSET,
        cProvParam: 0,
        rgProvParam: ptr::null_mut(),
        dwKeySpec: AT_SIGNATURE,
    };

    // Encode subject name "CN=MonsGeek Recovery"
    let mut name_size = 0u32;
    if CertStrToNameW(
        X509_ASN_ENCODING,
        subject.as_ptr(),
        CERT_X500_NAME_STR,
        ptr::null_mut(),
        ptr::null_mut(),
        &mut name_size,
        ptr::null_mut(),
    ) == 0
    {
        bail!("{}", win_err("CertStrToNameW (size)"));
    }

    let mut name_buf = vec![0u8; name_size as usize];
    if CertStrToNameW(
        X509_ASN_ENCODING,
        subject.as_ptr(),
        CERT_X500_NAME_STR,
        ptr::null_mut(),
        name_buf.as_mut_ptr(),
        &mut name_size,
        ptr::null_mut(),
    ) == 0
    {
        bail!("{}", win_err("CertStrToNameW"));
    }

    let name_blob = CRYPT_INTEGER_BLOB {
        cbData: name_size,
        pbData: name_buf.as_mut_ptr(),
    };

    // Build Code Signing EKU extension
    let mut eku_oid = OID_CODE_SIGNING.as_ptr() as *mut u8;
    let eku = CTL_USAGE {
        cUsageIdentifier: 1,
        rgpszUsageIdentifier: &mut eku_oid as *mut *mut u8,
    };

    // Encode the EKU — first pass gets size, second pass encodes
    let mut eku_encoded: *mut u8 = ptr::null_mut();
    let mut eku_size = 0u32;
    if CryptEncodeObjectEx(
        X509_ASN_ENCODING,
        X509_ENHANCED_KEY_USAGE_TYPE,
        &eku as *const _ as *const _,
        CRYPT_ENCODE_ALLOC_FLAG,
        ptr::null(),
        &mut eku_encoded as *mut _ as *mut _,
        &mut eku_size,
    ) == 0
    {
        bail!("{}", win_err("CryptEncodeObjectEx (EKU)"));
    }

    let mut extension = CERT_EXTENSION {
        pszObjId: OID_ENHANCED_KEY_USAGE.as_ptr() as *mut u8,
        fCritical: TRUE,
        Value: CRYPT_INTEGER_BLOB {
            cbData: eku_size,
            pbData: eku_encoded,
        },
    };

    let extensions = CERT_EXTENSIONS {
        cExtension: 1,
        rgExtension: &mut extension,
    };

    // SHA256 signature algorithm
    let sig_alg = CRYPT_ALGORITHM_IDENTIFIER {
        pszObjId: OID_SHA256_RSA.as_ptr() as *mut u8,
        Parameters: CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        },
    };

    // 10-year validity
    let start = SYSTEMTIME {
        wYear: 2025,
        wMonth: 1,
        wDayOfWeek: 0,
        wDay: 1,
        wHour: 0,
        wMinute: 0,
        wSecond: 0,
        wMilliseconds: 0,
    };
    let end = SYSTEMTIME {
        wYear: 2035,
        ..start
    };

    let cert = CertCreateSelfSignCertificate(
        0, // hProv = 0 → auto-create key via key_prov_info
        &name_blob,
        0,
        &key_prov_info,
        &sig_alg,
        &start,
        &end,
        &extensions,
    );

    // Free encoded EKU (allocated by CryptEncodeObjectEx with CRYPT_ENCODE_ALLOC_FLAG)
    if !eku_encoded.is_null() {
        LocalFree(eku_encoded as *mut _);
    }

    if cert.is_null() {
        bail!("{}", win_err("CertCreateSelfSignCertificate"));
    }

    Ok(cert)
}

/// Add a certificate to a machine-level certificate store.
unsafe fn install_cert_to_store(cert: *const CERT_CONTEXT, store_name: &str) -> Result<()> {
    let name = wide(store_name);
    let store = CertOpenStore(
        CERT_STORE_PROV_SYSTEM_W,
        0,
        0,
        CERT_SYSTEM_STORE_LOCAL_MACHINE,
        name.as_ptr() as *const _,
    );
    if store.is_null() {
        bail!("{}", win_err(&format!("CertOpenStore({})", store_name)));
    }

    let ok = CertAddCertificateContextToStore(
        store,
        cert,
        CERT_STORE_ADD_REPLACE_EXISTING,
        ptr::null_mut(),
    );
    CertCloseStore(store, 0);

    if ok == 0 {
        bail!(
            "{}",
            win_err(&format!(
                "CertAddCertificateContextToStore({})",
                store_name
            ))
        );
    }
    Ok(())
}

/// Remove our certificate from a machine-level certificate store.
unsafe fn remove_cert_from_store(cert: *const CERT_CONTEXT, store_name: &str) -> Result<()> {
    let name = wide(store_name);
    let store = CertOpenStore(
        CERT_STORE_PROV_SYSTEM_W,
        0,
        0,
        CERT_SYSTEM_STORE_LOCAL_MACHINE,
        name.as_ptr() as *const _,
    );
    if store.is_null() {
        return Ok(());
    }

    // Find our cert by matching its encoded form
    let found = CertFindCertificateInStore(
        store,
        X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
        0,
        CERT_FIND_EXISTING,
        cert as *const _,
        ptr::null(),
    );

    if !found.is_null() {
        // CertDeleteCertificateFromStore frees `found`
        CertDeleteCertificateFromStore(found);
    }

    CertCloseStore(store, 0);
    Ok(())
}

/// Delete the RSA key container used for signing.
unsafe fn delete_key_container() -> Result<()> {
    let container = wide(CONTAINER_NAME);
    let mut prov: usize = 0;
    // CRYPT_DELETE_KEYSET deletes the container; failure is OK (may not exist)
    CryptAcquireContextW(
        &mut prov,
        container.as_ptr(),
        ptr::null(),
        PROV_RSA_FULL,
        CRYPT_DELETE_KEYSET | CRYPT_MACHINE_KEYSET,
    );
    Ok(())
}

/// Create a .cat catalog file that references the INF.
unsafe fn create_catalog(inf_path: &Path, cat_path: &Path) -> Result<()> {
    let cat_path_w = wide(&cat_path.to_string_lossy());
    let inf_path_w = wide(&inf_path.to_string_lossy());

    // Create catalog file
    let hcat = CryptCATOpen(
        cat_path_w.as_ptr() as *mut u16,
        CRYPTCAT_OPEN_CREATENEW,
        0,
        CRYPTCAT_VERSION_1,
        0,
    );
    if hcat == INVALID_HANDLE_VALUE {
        bail!("{}", win_err("CryptCATOpen"));
    }

    let result = create_catalog_inner(hcat, &inf_path_w, inf_path);

    if result.is_ok() {
        log("    CryptCATPersistStore...");
        if CryptCATPersistStore(hcat) == 0 {
            let err = win_err("CryptCATPersistStore");
            log(&format!("    {err}"));
            CryptCATClose(hcat);
            bail!("{}", err);
        }
        log("    CryptCATPersistStore OK");
    }
    log("    CryptCATClose...");
    CryptCATClose(hcat);
    log("    CryptCATClose OK");

    result
}

unsafe fn create_catalog_inner(
    hcat: HANDLE,
    inf_path_w: &[u16],
    inf_path: &Path,
) -> Result<()> {
    // Add OS version attribute
    log("    Adding OS attribute...");
    let os_attr_name = wide("OS");
    let os_attr_value = wide("2:6.0,2:6.1,2:6.2,2:6.3,2:10.0");
    let attr = CryptCATPutCatAttrInfo(
        hcat,
        os_attr_name.as_ptr() as *mut u16,
        CRYPTCAT_ATTR_AUTHENTICATED | CRYPTCAT_ATTR_NAMEASCII | CRYPTCAT_ATTR_DATAASCII,
        (os_attr_value.len() * 2) as u32,
        os_attr_value.as_ptr() as *mut u8,
    );
    log(&format!("    OS attr result: {:?}", !attr.is_null()));

    // Hash the INF file using CryptCATAdmin
    log("    Hashing INF...");
    let inf_handle = CreateFileW(
        inf_path_w.as_ptr(),
        GENERIC_READ,
        FILE_SHARE_READ,
        ptr::null(),
        OPEN_EXISTING,
        0,
        ptr::null_mut(),
    );
    if inf_handle == INVALID_HANDLE_VALUE {
        bail!("{}", win_err("CreateFileW(INF)"));
    }

    let hash = compute_file_hash(inf_handle);
    CloseHandle(inf_handle);
    let hash = hash?;

    // Convert hash to uppercase hex string for the catalog member tag
    let hash_hex: String = hash.iter().map(|b| format!("{:02X}", b)).collect();
    let hash_hex_w = wide(&hash_hex);

    // Get INF filename (just the basename)
    let inf_filename = inf_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let inf_filename_w = wide(&inf_filename);

    // Build SIP_INDIRECT_DATA for the INF file (non-PE type)
    let mut hash_buf = hash;
    log(&format!("    Hash ({} bytes): {}", hash_buf.len(), &hash_hex));

    let mut sip_data = SIP_INDIRECT_DATA {
        Data: CRYPT_ATTRIBUTE_TYPE_VALUE {
            pszObjId: SPC_CAB_DATA_OBJID.as_ptr() as *mut u8,
            Value: CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: ptr::null_mut(),
            },
        },
        DigestAlgorithm: CRYPT_ALGORITHM_IDENTIFIER {
            pszObjId: OID_OIWSEC_SHA1.as_ptr() as *mut u8,
            Parameters: CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: ptr::null_mut(),
            },
        },
        Digest: CRYPT_INTEGER_BLOB {
            cbData: hash_buf.len() as u32,
            pbData: hash_buf.as_mut_ptr(),
        },
    };

    // Add INF as a catalog member
    log("    CryptCATPutMemberInfo...");
    let mut subject_guid = INF_SUBJECT_GUID;
    let member = CryptCATPutMemberInfo(
        hcat,
        ptr::null(), // pwszFileName = NULL for INF
        hash_hex_w.as_ptr() as *mut u16,
        &mut subject_guid,
        0x200,
        mem::size_of::<SIP_INDIRECT_DATA>() as u32,
        &mut sip_data as *mut _ as *mut u8,
    );

    if member.is_null() {
        bail!("{}", win_err("CryptCATPutMemberInfo"));
    }

    // Add "File" attribute to the member (filename reference)
    log("    CryptCATPutAttrInfo (File)...");
    let file_attr = wide("File");
    CryptCATPutAttrInfo(
        hcat,
        member,
        file_attr.as_ptr() as *mut u16,
        CRYPTCAT_ATTR_AUTHENTICATED | CRYPTCAT_ATTR_NAMEASCII | CRYPTCAT_ATTR_DATAASCII,
        (inf_filename_w.len() * 2) as u32,
        inf_filename_w.as_ptr() as *mut u8,
    );

    Ok(())
}

/// Compute the catalog hash of a file using CryptCATAdmin (SHA1 default).
unsafe fn compute_file_hash(file_handle: HANDLE) -> Result<Vec<u8>> {
    let mut admin: isize = 0;
    if CryptCATAdminAcquireContext(&mut admin, ptr::null(), 0) == 0 {
        bail!("{}", win_err("CryptCATAdminAcquireContext"));
    }

    // First call: get hash size
    let mut hash_size = 0u32;
    CryptCATAdminCalcHashFromFileHandle(file_handle, &mut hash_size, ptr::null_mut(), 0);

    // Second call: compute hash
    let mut hash = vec![0u8; hash_size as usize];
    let ok =
        CryptCATAdminCalcHashFromFileHandle(file_handle, &mut hash_size, hash.as_mut_ptr(), 0);
    CryptCATAdminReleaseContext(admin, 0);

    if ok == 0 {
        bail!("{}", win_err("CryptCATAdminCalcHashFromFileHandle"));
    }

    Ok(hash)
}

/// Sign a catalog file using the given certificate.
unsafe fn sign_catalog(cat_path: &Path, cert: *const CERT_CONTEXT) -> Result<()> {
    let cat_path_w = wide(&cat_path.to_string_lossy());

    let mut file_info = SIGNER_FILE_INFO {
        cbSize: mem::size_of::<SIGNER_FILE_INFO>() as u32,
        pwszFileName: cat_path_w.as_ptr(),
        hFile: ptr::null_mut(), // NULL → SignerSignEx opens the file
    };

    let mut index = 0u32;
    let mut subject_info: SIGNER_SUBJECT_INFO = mem::zeroed();
    subject_info.cbSize = mem::size_of::<SIGNER_SUBJECT_INFO>() as u32;
    subject_info.pdwIndex = &mut index;
    subject_info.dwSubjectChoice = SIGNER_SUBJECT_FILE;
    subject_info.Anonymous.pSignerFileInfo = &mut file_info;

    let mut store_info = SIGNER_CERT_STORE_INFO {
        cbSize: mem::size_of::<SIGNER_CERT_STORE_INFO>() as u32,
        pSigningCert: cert,
        dwCertPolicy: SIGNER_CERT_POLICY_CHAIN,
        hCertStore: ptr::null_mut(),
    };

    let mut signer_cert: SIGNER_CERT = mem::zeroed();
    signer_cert.cbSize = mem::size_of::<SIGNER_CERT>() as u32;
    signer_cert.dwCertChoice = SIGNER_CERT_STORE;
    signer_cert.Anonymous.pCertStoreInfo = &mut store_info;

    let mut sig_info: SIGNER_SIGNATURE_INFO = mem::zeroed();
    sig_info.cbSize = mem::size_of::<SIGNER_SIGNATURE_INFO>() as u32;
    sig_info.algidHash = CALG_SHA_256;
    sig_info.dwAttrChoice = SIGNER_NO_ATTR;

    let mut signer_ctx: *mut SIGNER_CONTEXT = ptr::null_mut();

    log(&format!(
        "    SignerSignEx: file_info.cbSize={}, subject_info.cbSize={}, cert.cbSize={}, sig.cbSize={}",
        file_info.cbSize, subject_info.cbSize, signer_cert.cbSize, sig_info.cbSize
    ));
    log("    Calling SignerSignEx...");

    let hr = SignerSignEx(
        0,
        &subject_info,
        &signer_cert,
        &sig_info,
        ptr::null(),     // provider info
        ptr::null(),     // timestamp URL
        ptr::null_mut(), // request
        ptr::null_mut(), // SIP data
        &mut signer_ctx,
    );

    log(&format!("    SignerSignEx returned HRESULT 0x{:08X}", hr as u32));

    if !signer_ctx.is_null() {
        SignerFreeSignerContext(signer_ctx);
    }

    if hr != 0 {
        bail!("SignerSignEx failed: HRESULT 0x{:08X}", hr as u32);
    }

    Ok(())
}

/// Install the driver using the signed INF.
unsafe fn install_driver(inf_path: &Path) -> Result<()> {
    let inf_path_w = wide(&inf_path.to_string_lossy());
    let hwid = wide("USB\\VID_2E3C&PID_DF11");

    let mut reboot: BOOL = FALSE;
    let ok = UpdateDriverForPlugAndPlayDevicesW(
        ptr::null_mut(), // NULL hwnd
        hwid.as_ptr(),
        inf_path_w.as_ptr(),
        INSTALLFLAG_FORCE,
        &mut reboot,
    );

    if ok != 0 {
        if reboot != 0 {
            log("  Note: A reboot may be required.");
        }
        return Ok(());
    }

    // Device may not be plugged in — fall back to pre-installing the driver
    let err = last_error_code();
    log(&format!(
        "  Device not found (0x{:08X}), pre-installing driver for later...",
        err
    ));

    if SetupCopyOEMInfW(
        inf_path_w.as_ptr(),
        ptr::null(),
        SPOST_PATH,
        0,
        ptr::null_mut(),
        0,
        ptr::null_mut(),
        ptr::null_mut(),
    ) == 0
    {
        bail!("{}", win_err("SetupCopyOEMInfW"));
    }

    log("  Driver pre-installed. It will activate when the DFU device appears.");
    Ok(())
}
