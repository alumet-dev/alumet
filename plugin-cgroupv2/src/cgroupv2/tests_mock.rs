#[cfg(test)]
use serde::Serialize;
use std::fs::File;
use std::io::{self, Read, Write};
use toml;

pub trait MockFileCgroupKV: Serialize {
    fn write_to_file(&self, mut file: File) -> io::Result<()> {
        let toml_str = toml::to_string(self).expect("TOML serialization failed");

        for line in toml_str.lines() {
            if let Some((key, value)) = line.split_once(" = ") {
                writeln!(file, "{} {}", key.trim(), value.trim_matches('"'))?;
            }
        }

        Ok(())
    }

    fn replace_to_file(&self, mut file: File) -> io::Result<()> {
        // Lire le contenu actuel du fichier pour pouvoir le modifier
        let mut file_content = String::new();
        file.read_to_string(&mut file_content)?;

        // Parser le contenu existant en un objet TOML (ici une Value)
        let mut parsed_toml: toml::Value = toml::de::from_str(&file_content).unwrap_or_else(|_| toml::Value::Table(Default::default()));

        // Sérialiser la structure `self` en TOML pour obtenir les nouvelles clés et valeurs
        let toml_str = toml::to_string(self).expect("TOML serialization failed");

        // Parser la nouvelle chaîne TOML
        let new_toml: toml::Value = toml::de::from_str(&toml_str).expect("TOML deserialization failed");

        // Fusionner les anciennes données avec les nouvelles
        match new_toml {
            toml::Value::Table(new_table) => {
                // Si nous avons un tableau (un objet de paires clé-valeur), on le fusionne
                let table = parsed_toml.as_table_mut().unwrap();
                for (key, value) in new_table {
                    table.insert(key, value);
                }
            }
            _ => {}
        }

        // Réécrire le contenu mis à jour dans le fichier en utilisant un espace entre clé et valeur
        let mut updated_toml = String::new();
        if let Some(table) = parsed_toml.as_table() {
            for (key, value) in table {
                // Convertir la valeur en chaîne
                let value_str = value.to_string();
                // Ecrire le couple clé-valeur avec un espace
                updated_toml.push_str(&format!("{} {}\n", key, value_str.trim_matches('"')));
            }
        }

        // Réinitialiser le fichier avant de réécrire son contenu
        file.set_len(0)?; // Réinitialiser le fichier à une longueur de 0
        file.write_all(updated_toml.as_bytes())?; // Écrire le contenu modifié dans le fichier

        Ok(())
    }
}

#[derive(Serialize, Debug, Default)]
pub struct CpuStatMock {
    pub usage_usec: u64,
    pub user_usec: u64,
    pub system_usec: u64,
    pub nr_periods: u64,
    pub nr_throttled: u64,
    pub throttled_usec: u64,
    pub nr_bursts: u64,
    pub burst_usec: u64,
}

impl MockFileCgroupKV for CpuStatMock {}

#[derive(Serialize, Debug, Default)]
pub struct MemoryStatMock {
    pub anon: u64,
    pub file: u64,
    pub kernel: u64,
    pub kernel_stack: u64,
    pub pagetables: u64,
    pub sec_pagetables: u64,
    pub percpu: u64,
    pub sock: u64,
    pub vmalloc: u64,
    pub shmem: u64,
    pub zswap: u64,
    pub zswapped: u64,
    pub file_mapped: u64,
    pub file_dirty: u64,
    pub file_writeback: u64,
    pub swapcached: u64,
    pub anon_thp: u64,
    pub file_thp: u64,
    pub shmem_thp: u64,
    pub inactive_anon: u64,
    pub active_anon: u64,
    pub inactive_file: u64,
    pub active_file: u64,
    pub unevictable: u64,
    pub slab_reclaimable: u64,
    pub slab_unreclaimable: u64,
    pub slab: u64,
    pub workingset_refault_anon: u64,
    pub workingset_refault_file: u64,
    pub workingset_activate_anon: u64,
    pub workingset_activate_file: u64,
    pub workingset_restore_anon: u64,
    pub workingset_restore_file: u64,
    pub workingset_nodereclaim: u64,
    pub pswpin: u64,
    pub pswpout: u64,
    pub pgscan: u64,
    pub pgsteal: u64,
    pub pgscan_kswapd: u64,
    pub pgscan_direct: u64,
    pub pgscan_khugepaged: u64,
    pub pgscan_proactive: u64,
    pub pgsteal_kswapd: u64,
    pub pgsteal_direct: u64,
    pub pgsteal_khugepaged: u64,
    pub pgsteal_proactive: u64,
    pub pgfault: u64,
    pub pgmajfault: u64,
    pub pgrefill: u64,
    pub pgactivate: u64,
    pub pgdeactivate: u64,
    pub pglazyfree: u64,
    pub pglazyfreed: u64,
    pub swpin_zero: u64,
    pub swpout_zero: u64,
    pub zswpin: u64,
    pub zswpout: u64,
    pub zswpwb: u64,
    pub thp_fault_alloc: u64,
    pub thp_collapse_alloc: u64,
    pub thp_swpout: u64,
    pub thp_swpout_fallback: u64,
    pub numa_pages_migrated: u64,
    pub numa_pte_updates: u64,
    pub numa_hint_faults: u64,
    pub pgdemote_kswapd: u64,
    pub pgdemote_direct: u64,
    pub pgdemote_khugepaged: u64,
    pub pgdemote_proactive: u64,
    pub hugetlb: u64,
}

impl MockFileCgroupKV for MemoryStatMock {}

#[derive(Serialize, Debug, Default)]
pub struct MemoryCurrentMock(pub u64);
impl MemoryCurrentMock {
    pub fn write_to_file(&self, mut file: File) -> io::Result<()> {
        writeln!(file, "{}", self.0)
    }
}
