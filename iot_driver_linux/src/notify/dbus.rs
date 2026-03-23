//! D-Bus interface for the notification daemon.
//!
//! Bus name: `org.monsgeek.Notify1`
//! Object path: `/org/monsgeek/Notify1`

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use zbus::interface;

use super::keymap;
use super::state::{Notification, NotificationStore};
use crate::effect::{self, EffectLibrary};

/// A delayed key reassignment for repeated keys in text animations.
#[derive(Debug, Clone)]
pub struct PendingWave {
    /// Matrix indices to assign.
    pub indices: Vec<usize>,
    /// Original slot numbers (for phase offset computation).
    pub slots: Vec<usize>,
    /// Stagger interval in ms per slot.
    pub stagger_ms: f64,
    /// Wall-clock time to send this wave.
    pub send_at: Instant,
    /// Effect name (for slot matching).
    pub effect_name: String,
    /// Compiled animation (for slot matching).
    pub compiled: crate::effect::CompiledAnim,
}

/// Queue of pending waves shared between D-Bus handler and daemon loop.
pub type PendingWaveQueue = Arc<Mutex<Vec<PendingWave>>>;

/// Shared state between D-Bus interface and render loop.
pub type SharedStore = Arc<Mutex<NotificationStore>>;

/// D-Bus interface implementation.
pub struct NotifyInterface {
    store: SharedStore,
    effects: Arc<EffectLibrary>,
    pending_waves: PendingWaveQueue,
}

impl NotifyInterface {
    pub fn new(
        store: SharedStore,
        effects: Arc<EffectLibrary>,
        pending_waves: PendingWaveQueue,
    ) -> Self {
        Self {
            store,
            effects,
            pending_waves,
        }
    }
}

#[interface(name = "org.monsgeek.Notify1")]
impl NotifyInterface {
    /// Post a notification. Returns notification ID.
    ///
    /// `vars` maps variable names to color values (e.g. {"color": "red"}).
    async fn notify(
        &self,
        source: &str,
        key: &str,
        effect_name: &str,
        priority: i32,
        ttl_ms: i32,
        vars: BTreeMap<String, String>,
    ) -> zbus::fdo::Result<u64> {
        let target = keymap::parse_key_target(key).map_err(zbus::fdo::Error::InvalidArgs)?;

        let def = self.effects.get(effect_name).ok_or_else(|| {
            zbus::fdo::Error::InvalidArgs(format!("unknown effect: {effect_name}"))
        })?;

        // Extract stagger before passing vars to effect resolver
        let mut effect_vars = vars;
        let stagger_ms: f64 = effect_vars
            .remove("stagger")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);

        let resolved = effect::resolve(def, &effect_vars).map_err(|e| {
            let required = effect::required_variables(def);
            zbus::fdo::Error::InvalidArgs(format!(
                "{e} (required variables: {})",
                required.join(", ")
            ))
        })?;

        // TTL: -1 = use effect default, 0 = no expiry, >0 = explicit ms
        let max_slot = target.slots.iter().max().copied().unwrap_or(0);
        let mut ttl = if ttl_ms > 0 {
            Some(Duration::from_millis(ttl_ms as u64))
        } else if ttl_ms == -1 {
            def.ttl_ms
                .filter(|&ms| ms > 0)
                .map(|ms| Duration::from_millis(ms as u64))
        } else {
            None
        };
        // Extend TTL to cover the full stagger span
        if stagger_ms > 0.0 {
            let ext_ms = max_slot as f64 * stagger_ms;
            ttl = ttl.map(|t| t + Duration::from_secs_f64(ext_ms / 1000.0));
        }

        // Split into waves for repeated keys
        let waves = split_into_waves(&target.indices, &target.slots);
        let (wave1_indices, wave1_slots) = &waves[0];

        // Wave 1: only unique keys, posted to store for daemon programming
        let stagger_offsets = build_stagger_offsets(wave1_indices, wave1_slots, stagger_ms);
        let notif = Notification {
            id: 0,
            source: source.to_string(),
            effect_name: effect_name.to_string(),
            matrix_indices: wave1_indices.clone(),
            resolved: resolved.clone(),
            priority,
            ttl,
            created: Instant::now(),
            stagger_offsets,
        };

        let mut store = self.store.lock().await;
        let id = store.add(notif);
        drop(store);

        // Waves 2+: enqueue for daemon to send via direct anim_assign
        if waves.len() > 1 {
            let one_shot = ttl.is_some() && resolved.duration_ms > 0.0;
            let pri = priority.clamp(-128, 127) as i8;
            if let Some(compiled) = resolved.compile_for_firmware(pri, one_shot) {
                let now = Instant::now();
                let mut queue = self.pending_waves.lock().await;
                for (indices, slots) in waves.into_iter().skip(1) {
                    let first_slot = slots.iter().min().copied().unwrap_or(0);
                    let delay = Duration::from_secs_f64(first_slot as f64 * stagger_ms / 1000.0);
                    queue.push(PendingWave {
                        indices,
                        slots,
                        stagger_ms,
                        send_at: now + delay,
                        effect_name: effect_name.to_string(),
                        compiled: compiled.clone(),
                    });
                }
            }
        }

        Ok(id)
    }

    /// Acknowledge (dismiss) a notification by ID.
    async fn acknowledge(&self, id: u64) -> zbus::fdo::Result<()> {
        let mut store = self.store.lock().await;
        store.remove(id);
        Ok(())
    }

    /// Acknowledge all notifications on a key.
    async fn acknowledge_key(&self, key: &str) -> zbus::fdo::Result<()> {
        let target = keymap::parse_key_target(key).map_err(zbus::fdo::Error::InvalidArgs)?;
        let mut store = self.store.lock().await;
        store.remove_by_key(&target.indices);
        Ok(())
    }

    /// Acknowledge all notifications from a source.
    async fn acknowledge_source(&self, source: &str) -> zbus::fdo::Result<()> {
        let mut store = self.store.lock().await;
        store.remove_by_source(source);
        Ok(())
    }

    /// List active notifications: Vec<(id, key, source, effect, priority)>.
    async fn list(&self) -> Vec<(u64, String, String, String, i32)> {
        let store = self.store.lock().await;
        store.list()
    }

    /// Clear all notifications.
    async fn clear(&self) -> zbus::fdo::Result<()> {
        let mut store = self.store.lock().await;
        store.clear();
        Ok(())
    }
}

/// Split indices+slots into waves where no key index repeats within a wave.
/// Each wave is a `(indices, slots)` pair preserving original slot numbers.
fn split_into_waves(indices: &[usize], slots: &[usize]) -> Vec<(Vec<usize>, Vec<usize>)> {
    let mut waves: Vec<(Vec<usize>, Vec<usize>)> = Vec::new();

    for (&idx, &slot) in indices.iter().zip(slots) {
        // Find the first wave that doesn't already contain this key
        let wave_idx = waves
            .iter()
            .position(|(idxs, _)| !idxs.contains(&idx))
            .unwrap_or_else(|| {
                waves.push((Vec::new(), Vec::new()));
                waves.len() - 1
            });
        waves[wave_idx].0.push(idx);
        waves[wave_idx].1.push(slot);
    }

    waves
}

fn build_stagger_offsets(
    indices: &[usize],
    slots: &[usize],
    stagger_ms: f64,
) -> HashMap<usize, f64> {
    if stagger_ms <= 0.0 {
        return HashMap::new();
    }
    indices
        .iter()
        .zip(slots)
        .map(|(&idx, &slot)| (idx, slot as f64 * stagger_ms))
        .collect()
}
