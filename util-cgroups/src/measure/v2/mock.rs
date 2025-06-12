use super::serde_util::{Impossible, SerializationError};
use std::{fmt::Display, fs::File, io::Seek};

use serde::{Serialize, ser::Serializer};
use thiserror::Error;

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

pub trait MockFileCgroupKV {
    fn serialize_to_string(&self) -> anyhow::Result<String>;

    fn write_to_file(&self, file: &mut File) -> anyhow::Result<()> {
        use std::io::Write;
        file.rewind()?;
        writeln!(file, "{}", self.serialize_to_string()?)?;
        file.flush()?;
        Ok(())
    }
}

impl<S: Serialize> MockFileCgroupKV for S {
    fn serialize_to_string(&self) -> anyhow::Result<String> {
        let res = self.serialize(StatSerializer)?;
        Ok(res)
    }
}

pub struct StatSerializer;

impl Serializer for StatSerializer {
    type Ok = String;
    type Error = SerializationError;

    type SerializeSeq = Impossible<String, Self::Error>;
    type SerializeTuple = Impossible<String, Self::Error>;
    type SerializeTupleStruct = Impossible<String, Self::Error>;
    type SerializeTupleVariant = Impossible<String, Self::Error>;
    type SerializeMap = Impossible<String, Self::Error>;
    type SerializeStruct = SerializeStructWrapper;
    type SerializeStructVariant = Impossible<String, Self::Error>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_some<T: ?Sized>(self, _value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: serde::Serialize,
    {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_newtype_struct<T: ?Sized>(self, _name: &'static str, _value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: serde::Serialize,
    {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_newtype_variant<T: ?Sized>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: serde::Serialize,
    {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(SerializeStructWrapper {
            fields: Vec::with_capacity(len),
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }
}

pub struct SerializeStructWrapper {
    fields: Vec<String>,
}

impl serde::ser::SerializeStruct for SerializeStructWrapper {
    type Ok = String;
    type Error = SerializationError;

    fn serialize_field<T: ?Sized>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: serde::Serialize,
    {
        let value_str = value.serialize(StatSerializer)?;
        self.fields.push(format!("{key} {value_str}"));
        Ok(())
    }

    fn end(self) -> Result<String, Self::Error> {
        Ok(self.fields.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn serialize_mock() {
        let mock = CpuStatMock {
            usage_usec: 0,
            user_usec: 1,
            system_usec: 123,
            nr_periods: 123,
            nr_throttled: 123,
            throttled_usec: 123,
            nr_bursts: 123,
            burst_usec: 123456789,
        };
        let s = mock.serialize_to_string().unwrap();
        const EXPECTED: &str = "usage_usec 0
user_usec 1
system_usec 123
nr_periods 123
nr_throttled 123
throttled_usec 123
nr_bursts 123
burst_usec 123456789";
        assert_eq!(s, EXPECTED);
    }
}
