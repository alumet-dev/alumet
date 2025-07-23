use super::serde_util::{Impossible, SerializationError};
use std::{fs::File, io::Seek};

use serde::{ser::Serializer, Serialize};

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

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: serde::Serialize + ?Sized,
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

    #[test]
    fn test_serialize_bool() {
        let serializer = StatSerializer;
        // Test with true
        let true_serialized_res = serializer.serialize_bool(true);
        assert!(true_serialized_res.is_ok());
        let true_serialized = true_serialized_res.unwrap();
        assert_eq!(true_serialized, "true");

        let serializer = StatSerializer;
        // Test with false
        let false_serialized_res = serializer.serialize_bool(false);
        assert!(false_serialized_res.is_ok());
        let false_serialized = false_serialized_res.unwrap();
        assert_eq!(false_serialized, "false");
    }

    #[test]
    fn test_serialize_i8() {
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_i8(15 as i8);
        assert!(serialized_res.is_ok());
        let true_serialized = serialized_res.unwrap();
        assert_eq!(true_serialized, "15");

        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_i8(-19 as i8);
        assert!(serialized_res.is_ok());
        let false_serialized = serialized_res.unwrap();
        assert_eq!(false_serialized, "-19");
    }

    #[test]
    fn test_serialize_i16() {
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_i16(150 as i16);
        assert!(serialized_res.is_ok());
        let true_serialized = serialized_res.unwrap();
        assert_eq!(true_serialized, "150");

        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_i16(-190 as i16);
        assert!(serialized_res.is_ok());
        let false_serialized = serialized_res.unwrap();
        assert_eq!(false_serialized, "-190");
    }

    #[test]
    fn test_serialize_i32() {
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_i32(12 as i32);
        assert!(serialized_res.is_ok());
        let true_serialized = serialized_res.unwrap();
        assert_eq!(true_serialized, "12");

        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_i32(-10 as i32);
        assert!(serialized_res.is_ok());
        let false_serialized = serialized_res.unwrap();
        assert_eq!(false_serialized, "-10");
    }

    #[test]
    fn test_serialize_i64() {
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_i64(0 as i64);
        assert!(serialized_res.is_ok());
        let true_serialized = serialized_res.unwrap();
        assert_eq!(true_serialized, "0");

        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_i64(-98 as i64);
        assert!(serialized_res.is_ok());
        let false_serialized = serialized_res.unwrap();
        assert_eq!(false_serialized, "-98");
    }

    #[test]
    fn test_serialize_u8() {
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_u8(150 as u8);
        assert!(serialized_res.is_ok());
        let true_serialized = serialized_res.unwrap();
        assert_eq!(true_serialized, "150");

        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_u8(0 as u8);
        assert!(serialized_res.is_ok());
        let false_serialized = serialized_res.unwrap();
        assert_eq!(false_serialized, "0");
    }

    #[test]
    fn test_serialize_u16() {
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_u16(1550 as u16);
        assert!(serialized_res.is_ok());
        let true_serialized = serialized_res.unwrap();
        assert_eq!(true_serialized, "1550");

        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_u16(0 as u16);
        assert!(serialized_res.is_ok());
        let false_serialized = serialized_res.unwrap();
        assert_eq!(false_serialized, "0");
    }

    #[test]
    fn test_serialize_u32() {
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_u32(191512 as u32);
        assert!(serialized_res.is_ok());
        let true_serialized = serialized_res.unwrap();
        assert_eq!(true_serialized, "191512");

        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_u32(0 as u32);
        assert!(serialized_res.is_ok());
        let false_serialized = serialized_res.unwrap();
        assert_eq!(false_serialized, "0");
    }

    #[test]
    fn test_serialize_u64() {
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_u64(8952 as u64);
        assert!(serialized_res.is_ok());
        let true_serialized = serialized_res.unwrap();
        assert_eq!(true_serialized, "8952");

        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_u64(0 as u64);
        assert!(serialized_res.is_ok());
        let false_serialized = serialized_res.unwrap();
        assert_eq!(false_serialized, "0");
    }

    #[test]
    fn test_serialize_f32() {
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_f32(3.14 as f32);
        assert!(serialized_res.is_ok());
        let true_serialized = serialized_res.unwrap();
        assert_eq!(true_serialized, "3.14");

        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_f32(-273.15 as f32);
        assert!(serialized_res.is_ok());
        let false_serialized = serialized_res.unwrap();
        assert_eq!(false_serialized, "-273.15");
    }

    #[test]
    fn test_serialize_f64() {
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_f64(3.14 as f64);
        assert!(serialized_res.is_ok());
        let true_serialized = serialized_res.unwrap();
        assert_eq!(true_serialized, "3.14");

        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_f64(-273.15 as f64);
        assert!(serialized_res.is_ok());
        let false_serialized = serialized_res.unwrap();
        assert_eq!(false_serialized, "-273.15");
    }

    #[test]
    fn test_serialize_char() {
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_char('c');
        assert!(serialized_res.is_ok());
        let true_serialized = serialized_res.unwrap();
        assert_eq!(true_serialized, "c");

        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_char('@');
        assert!(serialized_res.is_ok());
        let false_serialized = serialized_res.unwrap();
        assert_eq!(false_serialized, "@");
    }

    #[test]
    fn test_serialize_str() {
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_str("this is str");
        assert!(serialized_res.is_ok());
        let true_serialized = serialized_res.unwrap();
        assert_eq!(true_serialized, "this is str");

        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_str("an other str");
        assert!(serialized_res.is_ok());
        let false_serialized = serialized_res.unwrap();
        assert_eq!(false_serialized, "an other str");
    }

    #[test]
    fn test_serialize_bytes() {
        let serializer = StatSerializer;
        let byte_array: [u8; 5] = [0, 1, 2, 3, 4];
        let serialized_res = serializer.serialize_bytes(&byte_array);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        }
    }

    #[test]
    fn test_serialize_none() {
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_none();
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        }
    }

    #[test]
    fn test_serialize_some() {
        #[derive(Serialize)]
        enum People {
            _Claudius,
            _Marcus,
            _Meto,
            _Paulus,
            _Remus,
            Romulus,
        }
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_some(&People::Romulus);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        }
    }

    #[test]
    fn test_serialize_unit() {
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_unit();
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        }
    }

    #[test]
    fn test_serialize_unit_struct() {
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_unit_struct("name");
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        }
    }

    #[test]
    fn test_serialize_unit_variant() {
        let serializer = StatSerializer;
        let serialized_res = serializer.serialize_unit_variant("name", 1 as u32, "variant");
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        }
    }

    #[test]
    fn test_serialize_newtype_struct() {
        let serializer = StatSerializer;
        let value = 12 as i32;
        let serialized_res = serializer.serialize_newtype_struct("name", &value);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        }
    }

    #[test]
    fn test_serialize_newtype_variant() {
        let serializer = StatSerializer;
        let arr: [i32; 3] = [1, 2, 3];
        let serialized_res = serializer.serialize_newtype_variant("name", 18 as u32, "variant", &arr);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        }
    }

    #[test]
    fn test_serialize_seq() {
        let serializer = StatSerializer;
        let value = 500 as usize;
        let serialized_res = serializer.serialize_seq(Some(value));
        assert!(serialized_res.is_err());
    }

    #[test]
    fn test_serialize_tuple() {
        let serializer = StatSerializer;
        let value = 500 as usize;
        let serialized_res = serializer.serialize_tuple(value);
        assert!(serialized_res.is_err());
    }

    #[test]
    fn test_serialize_tuple_struct() {
        let serializer = StatSerializer;
        let value = 500 as usize;
        let serialized_res = serializer.serialize_tuple_struct("name", value);
        assert!(serialized_res.is_err());
    }

    #[test]
    fn test_serialize_tuple_variant() {
        let serializer = StatSerializer;
        let value = 500 as usize;
        let serialized_res = serializer.serialize_tuple_variant("name", 14 as u32, "variant", value);
        assert!(serialized_res.is_err());
    }

    #[test]
    fn test_serialize_map() {
        let serializer = StatSerializer;
        let value = 500 as usize;
        let serialized_res = serializer.serialize_map(Some(value));
        assert!(serialized_res.is_err());
    }

    #[test]
    fn test_serialize_struct_variant() {
        let serializer = StatSerializer;
        let value = 500 as usize;
        let serialized_res = serializer.serialize_struct_variant("name", 5 as u32, "variant", value);
        assert!(serialized_res.is_err());
    }
}
