use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PairedPhone {
    pub(crate) phone_name: String,
    pub(crate) public_key_spki: String,
    #[serde(default)]
    pub(crate) paired_at: u64,
    #[serde(default)]
    pub(crate) last_connected: Option<u64>,
}

pub(crate) struct PairedPhoneStore {
    phones: Mutex<HashMap<String, PairedPhone>>,
    write: Mutex<()>,
}

impl PairedPhoneStore {
    pub(crate) fn load() -> Result<Self, Box<dyn Error>> {
        let path = phones_path()?;
        let stored = if path.exists() {
            serde_json::from_slice(&fs::read(&path)?)?
        } else {
            HashMap::new()
        };
        let (phones, changed) = deduplicate_paired_phones(stored);
        if changed {
            save_paired_phones(&phones)?;
        }
        Ok(Self {
            phones: Mutex::new(phones),
            write: Mutex::new(()),
        })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.phones
            .lock()
            .map(|phones| phones.is_empty())
            .unwrap_or(true)
    }

    pub(crate) fn snapshot(&self) -> Vec<(String, PairedPhone)> {
        let mut phones = self
            .phones
            .lock()
            .map(|phones| {
                phones
                    .iter()
                    .map(|(id, phone)| (id.clone(), phone.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        phones.sort_by(|left, right| left.1.phone_name.cmp(&right.1.phone_name));
        phones
    }

    pub(crate) fn public_key(&self, phone_id: &str) -> Option<String> {
        self.phones
            .lock()
            .ok()?
            .get(phone_id)
            .map(|phone| phone.public_key_spki.clone())
    }

    pub(crate) fn upsert(
        &self,
        phone_id: &str,
        phone_name: &str,
        public_key_spki: &str,
        now: u64,
    ) -> Result<(), Box<dyn Error>> {
        self.update(|phones| {
            upsert_paired_phone(phones, phone_id, phone_name, public_key_spki, now);
        })
    }

    pub(crate) fn mark_connected(
        &self,
        phone_id: &str,
        phone_name: &str,
        now: u64,
    ) -> Result<(), Box<dyn Error>> {
        self.update(|phones| {
            let phone = phones.get_mut(phone_id).ok_or("phone is not paired")?;
            phone.phone_name = phone_name.to_owned();
            phone.last_connected = Some(now);
            Ok::<_, Box<dyn Error>>(())
        })?
    }

    pub(crate) fn remove(&self, phone_id: &str) -> Result<(), Box<dyn Error>> {
        self.update(|phones| {
            phones.remove(phone_id);
        })
    }

    fn update<R>(
        &self,
        operation: impl FnOnce(&mut HashMap<String, PairedPhone>) -> R,
    ) -> Result<R, Box<dyn Error>> {
        let _write = self.write.lock().map_err(|_| "phone store unavailable")?;
        let mut phones = self
            .phones
            .lock()
            .map_err(|_| "phone store unavailable")?
            .clone();
        let result = operation(&mut phones);
        save_paired_phones(&phones)?;
        *self.phones.lock().map_err(|_| "phone store unavailable")? = phones;
        Ok(result)
    }
}

fn phones_path() -> Result<std::path::PathBuf, std::io::Error> {
    Ok(crate::identity::data_dir()?.join("paired-phones-v2.json"))
}

fn save_paired_phones(phones: &HashMap<String, PairedPhone>) -> Result<(), Box<dyn Error>> {
    crate::atomic_file::write(&phones_path()?, &serde_json::to_vec(phones)?)?;
    Ok(())
}

pub(crate) fn deduplicate_paired_phones(
    phones: HashMap<String, PairedPhone>,
) -> (HashMap<String, PairedPhone>, bool) {
    let original_len = phones.len();
    let mut entries = phones.into_iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        let left_time = (left.1.last_connected.unwrap_or(0), left.1.paired_at);
        let right_time = (right.1.last_connected.unwrap_or(0), right.1.paired_at);
        right_time
            .cmp(&left_time)
            .then_with(|| left.0.cmp(&right.0))
    });

    let mut seen_keys = HashSet::new();
    let mut deduplicated = HashMap::with_capacity(entries.len());
    for (phone_id, phone) in entries {
        if phone.public_key_spki.is_empty() || seen_keys.insert(phone.public_key_spki.clone()) {
            deduplicated.insert(phone_id, phone);
        }
    }
    let changed = deduplicated.len() != original_len;
    (deduplicated, changed)
}

pub(crate) fn upsert_paired_phone(
    phones: &mut HashMap<String, PairedPhone>,
    phone_id: &str,
    phone_name: &str,
    public_key_spki: &str,
    now: u64,
) {
    if let Some(phone) = phones.get_mut(phone_id) {
        phone.phone_name = phone_name.to_owned();
        phone.public_key_spki = public_key_spki.to_owned();
        phone.last_connected = Some(now);
        if phone.paired_at == 0 {
            phone.paired_at = now;
        }
        return;
    }

    phones.insert(
        phone_id.to_owned(),
        PairedPhone {
            phone_name: phone_name.to_owned(),
            public_key_spki: public_key_spki.to_owned(),
            paired_at: now,
            last_connected: Some(now),
        },
    );
}
