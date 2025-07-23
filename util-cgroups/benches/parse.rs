#![allow(unused)]

use std::collections::HashSet;

use criterion::BenchmarkId;

use crate::alternatives::{
    parse_space_kv_unchecked, parse_space_kv_unchecked_cached_indices, parse_space_kv_utf8, parse_space_kv_utf8_basic,
    parse_space_kv_utf8_cached_indices, BitSet128, BitSet64, IndexCache,
};

use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;

const MEMORY_STAT: &str = "anon 9202163712
file 5392793600
kernel 446648320
kernel_stack 25067520
pagetables 91107328
sec_pagetables 0
percpu 2989240
sock 8192
vmalloc 2240512
shmem 1537892352
zswap 0
zswapped 0
file_mapped 1073401856
file_dirty 40960
file_writeback 0
swapcached 344064
anon_thp 0
file_thp 0
shmem_thp 0
inactive_anon 3904520192
active_anon 6295154688
inactive_file 2995503104
active_file 859398144
unevictable 540581888
slab_reclaimable 255575264
slab_unreclaimable 67153928
slab 322729192
workingset_refault_anon 0
workingset_refault_file 21412
workingset_activate_anon 0
workingset_activate_file 19239
workingset_restore_anon 0
workingset_restore_file 721
workingset_nodereclaim 512
pgscan 410384
pgsteal 388427
pgscan_kswapd 396436
pgscan_direct 13948
pgscan_khugepaged 0
pgsteal_kswapd 374618
pgsteal_direct 13809
pgsteal_khugepaged 0
pgfault 113489463
pgmajfault 20903
pgrefill 264470
pgactivate 1914
pgdeactivate 0
pglazyfree 0
pglazyfreed 0
zswpin 0
zswpout 0
zswpwb 0
thp_fault_alloc 0
thp_collapse_alloc 0
thp_swpout 0
thp_swpout_fallback 0
";
const CPU_STAT: &str = "usage_usec 12849502000
user_usec 10191064000
system_usec 2658438000
core_sched.force_idle_usec 0
nr_periods 0
nr_throttled 0
throttled_usec 0
nr_bursts 0
burst_usec 0
";

fn consume_line_memory_stat(key: &str, value: u64) {
    match key {
        "anon" | "file" | "kernel_stack" | "pagetables" => black_box(value),
        _ => black_box(value),
    };
}

fn consume_line_cpu_stat(key: &str, value: u64) {
    match key {
        "user_usec" | "system_usec" | "usage_usec" => black_box(value),
        _ => black_box(value),
    };
}

fn prepare_cache_memory_stat<C: IndexCache>() -> C {
    C::new(&[0, 1, 3, 4])
}

fn prepare_cache_memory_cpu<C: IndexCache>() -> C {
    C::new(&[0, 1, 2])
}

/*
BENCHMARK RESULTS on my machine (Lenovo Thinkpad Gen1):
- basic version (previous cgroupv2 plugin): 2.83 µs [2.8105 µs 2.8290 µs 2.8517 µs]
- new optimized version unchecked+cached  : 0.93 µs [928.17 ns 931.69 ns 936.35 ns]

utf8_basic              time:   [2.8105 µs 2.8290 µs 2.8517 µs]
Found 11 outliers among 100 measurements (11.00%)
  3 (3.00%) high mild
  8 (8.00%) high severe

utf8                    time:   [1.7825 µs 1.7948 µs 1.8097 µs]
Found 17 outliers among 100 measurements (17.00%)
  5 (5.00%) high mild
  12 (12.00%) high severe

unchecked               time:   [1.8458 µs 1.8511 µs 1.8574 µs]
Found 14 outliers among 100 measurements (14.00%)
  6 (6.00%) high mild
  8 (8.00%) high severe

unchecked+cache/Vec     time:   [1.1143 µs 1.1167 µs 1.1203 µs]
Found 16 outliers among 100 measurements (16.00%)
  1 (1.00%) low mild
  4 (4.00%) high mild
  11 (11.00%) high severe

unchecked+cache/HashSet time:   [1.6788 µs 1.6817 µs 1.6856 µs]
Found 11 outliers among 100 measurements (11.00%)
  3 (3.00%) high mild
  8 (8.00%) high severe

unchecked+cache/BitSet64
                        time:   [928.17 ns 931.69 ns 936.35 ns]
Found 16 outliers among 100 measurements (16.00%)
  4 (4.00%) high mild
  12 (12.00%) high severe

unchecked+cache/BitSet128
                        time:   [966.94 ns 970.46 ns 974.89 ns]
Found 16 outliers among 100 measurements (16.00%)
  6 (6.00%) high mild
  10 (10.00%) high severe

utf8+cache/BitSet64     time:   [1.0241 µs 1.0272 µs 1.0305 µs]
Found 11 outliers among 100 measurements (11.00%)
  3 (3.00%) high mild
  8 (8.00%) high severe
*/

pub fn criterion_benchmark(c: &mut Criterion) {
    let io_buf = MEMORY_STAT.as_bytes();

    // === without the index cache ==
    c.bench_function("utf8_basic", |b| {
        b.iter(|| parse_space_kv_utf8_basic(io_buf, consume_line_memory_stat))
    });
    c.bench_function("utf8", |b| {
        b.iter(|| parse_space_kv_utf8(io_buf, consume_line_memory_stat))
    });
    c.bench_function("unchecked", |b| {
        b.iter(|| parse_space_kv_unchecked(io_buf, consume_line_memory_stat))
    });

    // === with an index cache ===
    let cache_vec = prepare_cache_memory_stat::<Vec<usize>>();
    let cache_hash = prepare_cache_memory_stat::<HashSet<usize>>();
    let cache_b64 = prepare_cache_memory_stat::<BitSet64>();
    let cache_b128 = prepare_cache_memory_stat::<BitSet128>();

    c.bench_with_input(BenchmarkId::new("unchecked+cache", "Vec"), &cache_vec, |b, indices| {
        b.iter(|| {
            parse_space_kv_unchecked_cached_indices(io_buf, indices, |a, b| {
                black_box((a, b));
            })
        });
    });
    c.bench_with_input(
        BenchmarkId::new("unchecked+cache", "HashSet"),
        &cache_hash,
        |b, indices| {
            b.iter(|| {
                parse_space_kv_unchecked_cached_indices(io_buf, indices, |a, b| {
                    black_box((a, b));
                })
            });
        },
    );
    c.bench_with_input(
        BenchmarkId::new("unchecked+cache", "BitSet64"),
        &cache_b64,
        |b, indices| {
            b.iter(|| {
                parse_space_kv_unchecked_cached_indices(io_buf, indices, |a, b| {
                    black_box((a, b));
                })
            });
        },
    );
    c.bench_with_input(
        BenchmarkId::new("unchecked+cache", "BitSet128"),
        &cache_b128,
        |b, indices| {
            b.iter(|| {
                parse_space_kv_unchecked_cached_indices(io_buf, indices, |a, b| {
                    black_box((a, b));
                })
            });
        },
    );
    c.bench_with_input(BenchmarkId::new("utf8+cache", "BitSet64"), &cache_b64, |b, indices| {
        b.iter(|| {
            parse_space_kv_utf8_cached_indices(io_buf, indices, |a, b| {
                black_box((a, b));
            })
        });
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);

mod alternatives {
    use std::{collections::HashSet, io};

    pub fn parse_space_kv_unchecked_cached_indices(
        io_buf: &[u8],
        indices: &impl IndexCache,
        mut on_kv: impl FnMut(&str, u64),
    ) -> io::Result<()> {
        let content = unsafe { std::str::from_utf8_unchecked(io_buf) };
        for (i, line) in content.split('\n').enumerate() {
            if indices.contains(i) {
                if let Some((key, value)) = line.split_once(' ') {
                    let value: u64 = value.parse().map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
                    on_kv(key, value)
                }
            }
        }
        Ok(())
    }

    pub fn parse_space_kv_utf8_cached_indices(
        io_buf: &[u8],
        indices: &impl IndexCache,
        mut on_kv: impl FnMut(&str, u64),
    ) -> io::Result<()> {
        let content = std::str::from_utf8(io_buf).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        for (i, line) in content.split('\n').enumerate() {
            if indices.contains(i) {
                if let Some((key, value)) = line.split_once(' ') {
                    let value: u64 = value.parse().map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
                    on_kv(key, value)
                }
            }
        }
        Ok(())
    }

    pub fn parse_space_kv_unchecked(io_buf: &[u8], mut on_kv: impl FnMut(&str, u64)) -> io::Result<()> {
        let content = unsafe { std::str::from_utf8_unchecked(io_buf) };
        for line in content.split('\n') {
            if let Some((key, value)) = line.split_once(' ') {
                let value: u64 = value.parse().map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
                on_kv(key, value)
            }
        }
        Ok(())
    }

    pub fn parse_space_kv_utf8(io_buf: &[u8], mut on_kv: impl FnMut(&str, u64)) -> io::Result<()> {
        let content = std::str::from_utf8(io_buf).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        for line in content.split('\n') {
            if let Some((key, value)) = line.split_once(' ') {
                let value: u64 = value.parse().map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
                on_kv(key, value)
            }
        }
        Ok(())
    }

    pub fn parse_space_kv_utf8_basic(io_buf: &[u8], mut on_kv: impl FnMut(&str, u64)) -> io::Result<()> {
        let content = std::str::from_utf8(io_buf).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        for line in content.lines() {
            let parts = line.split_ascii_whitespace().collect::<Vec<_>>();
            if parts.len() < 2 {
                return Err(io::Error::from(io::ErrorKind::InvalidData));
            }
            let key = parts[0];
            let value: u64 = parts[1]
                .parse()
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
            on_kv(key, value)
        }
        Ok(())
    }

    pub trait IndexCache {
        fn new(set: &[usize]) -> Self;
        fn contains(&self, i: usize) -> bool;
    }

    impl IndexCache for HashSet<usize> {
        fn new(set: &[usize]) -> Self {
            HashSet::from_iter(set.iter().cloned())
        }

        fn contains(&self, i: usize) -> bool {
            HashSet::contains(self, &i)
        }
    }

    impl IndexCache for Vec<usize> {
        fn new(set: &[usize]) -> Self {
            Vec::from_iter(set.iter().cloned())
        }

        fn contains(&self, i: usize) -> bool {
            <[usize]>::contains(self, &i)
        }
    }

    pub struct BitSet64(u64);
    pub struct BitSet128(u128);

    impl IndexCache for BitSet64 {
        fn new(set: &[usize]) -> Self {
            let mut v = 0;
            for i in set {
                v |= (1 << i);
            }
            Self(v)
        }

        fn contains(&self, i: usize) -> bool {
            self.0 & (1 << i) != 0
        }
    }

    impl IndexCache for BitSet128 {
        fn contains(&self, i: usize) -> bool {
            self.0 & (1 << i) != 0
        }

        fn new(set: &[usize]) -> Self {
            let mut v = 0;
            for i in set {
                v |= (1 << i);
            }
            Self(v)
        }
    }

    // #[cfg(test)]
    // mod tests {
    //     use crate::alternatives::{BitSet128, BitSet64};

    //     #[test]
    //     fn test_b64() {
    //         let b64 = BitSet64::new(&[]);
    //         for i in 0..64 {
    //             assert!(!b64.contains(i));
    //         }

    //         let b64 = BitSet64::new(&[1, 2, 5, 7]);
    //         println!("b64: {:b}", b64.0);
    //         assert!(b64.contains(1));
    //         assert!(b64.contains(2));
    //         assert!(!b64.contains(3));
    //         assert!(!b64.contains(4));
    //         assert!(b64.contains(5));
    //         assert!(!b64.contains(6));
    //         assert!(b64.contains(7));
    //     }

    //     #[test]
    //     fn test_b128() {
    //         let b128 = BitSet128::new(&[]);
    //         for i in 0..128 {
    //             assert!(!b128.contains(i));
    //         }

    //         let b128 = BitSet128::new(&[1, 2, 5, 7]);
    //         assert!(b128.contains(1));
    //         assert!(b128.contains(2));
    //         assert!(!b128.contains(3));
    //         assert!(!b128.contains(4));
    //         assert!(b128.contains(5));
    //         assert!(!b128.contains(6));
    //         assert!(b128.contains(7));
    //     }
    // }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alternatives::*;

    #[test]
    fn test_prepare_cache_memory_stat() {
        let cache = prepare_cache_memory_stat::<MockCache>();
        assert_eq!(cache.get_indices(), vec![0, 1, 3, 4]);
    }

    #[test]
    fn test_prepare_cache_memory_cpu() {
        let cache = prepare_cache_memory_cpu::<MockCache>();
        assert_eq!(cache.get_indices(), vec![0, 1, 2]);
    }

    #[test]
    fn test_parse_space_kv_unchecked_cached_indices_hashset() {
        let data = b"a1 111\nb2 222\nc3 333\n";
        let indices: HashSet<usize> = IndiceCache::new(&[0, 2]);
        let mut results = Vec::new();

        parse_space_kv_unchecked_cached_indices(data, &indices, |key, value| {
            results.push((key.to_string(), value));
        })
        .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0], ("key1".to_string(), 111));
        assert_eq!(results[1], ("key3".to_string(), 333));
    }

    #[test]
    fn test_parse_space_kv_unchecked_cached_indices_vec() {
        let data = b"key1 321\nkey2 123\nkey3 231\n";
        let indices: Vec<usize> = IndiceCache::new(&[1]);
        let mut results = Vec::new();

        parse_space_kv_unchecked_cached_indices(data, &indices, |key, value| {
            results.push((key.to_string(), value));
        })
        .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0], ("key2".to_string(), 123));
    }

    #[test]
    fn test_parse_space_kv_utf8_invalid_number() {
        let data = b"key1 100\nkey2 invalid_number\nkey3 300\n";
        let indices: HashSet<usize> = IndiceCache::new(&[0, 1, 2]);
        let mut results = Vec::new();

        let result = parse_space_kv_utf8_cached_indices(data, &indices, |key, value| {
            results.push((key.to_string(), value));
        });

        assert!(result.is_err()); // Expect an error due to invalid number
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData); // Check for specific error kind
    }

    #[test]
    fn test_parse_space_kv_utf8_cached_indices_hashset() {
        let data = b"a1 111\nb2 222\nc3 333\n";
        let indices: HashSet<usize> = IndiceCache::new(&[0, 2]);
        let mut results = Vec::new();

        parse_space_kv_utf8_cached_indices(data, &indices, |key, value| {
            results.push((key.to_string(), value));
        })
        .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0], ("a1".to_string(), 111));
        assert_eq!(results[1], ("c3".to_string(), 333));
    }

    #[test]
    fn test_parse_space_kv_utf8_cached_indices_vec() {
        let data = b"key1 321\nkey2 123\nkey3 231\n";
        let indices: Vec<usize> = IndiceCache::new(&[1]);
        let mut results = Vec::new();

        parse_space_kv_utf8_cached_indices(data, &indices, |key, value| {
            results.push((key.to_string(), value));
        })
        .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0], ("key2".to_string(), 123));
    }

    #[test]
    fn test_parse_space_kv_utf8_invalid_number() {
        let data = b"key1 100\nkey2 invalid_number\nkey3 300\n";
        let indices: HashSet<usize> = IndiceCache::new(&[0, 1, 2]);
        let mut results = Vec::new();

        let result = parse_space_kv_utf8_cached_indices(data, &indices, |key, value| {
            results.push((key.to_string(), value));
        });

        assert!(result.is_err()); // Expect an error due to invalid number
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData); // Check for specific error kind
    }

    #[test]
    fn test_parse_space_kv_utf8_empty_input() {
        let data = b"";
        let indices: HashSet<usize> = IndiceCache::new(&[]);
        let mut results = Vec::new();

        parse_space_kv_utf8_cached_indices(data, &indices, |key, value| {
            results.push((key.to_string(), value));
        })
        .unwrap();

        assert!(results.is_empty()); // Expect an empty result
    }

    #[test]
    fn test_parse_space_kv_utf8_out_of_bounds_index() {
        let data = b"key1 100\nkey2 200\nkey3 300\n";
        let indices: Vec<usize> = IndiceCache::new(&[3]); // Out of bounds
        let mut results = Vec::new();

        parse_space_kv_utf8_cached_indices(data, &indices, |key, value| {
            results.push((key.to_string(), value));
        })
        .unwrap();

        assert!(results.is_empty()); // Expect an empty result
    }

    #[test]
    fn test_parse_space_kv_utf8_multiple_pairs_on_one_line() {
        let data = b"key1 100 key2 200\nkey3 300\n";
        let indices: HashSet<usize> = IndiceCache::new(&[0]); // Only process the first line
        let mut results = Vec::new();

        parse_space_kv_utf8_cached_indices(data, &indices, |key, value| {
            results.push((key.to_string(), value));
        })
        .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0], ("key1".to_string(), 100)); // Only the first pair should be processed
    }

    #[test]
    fn test_parse_space_kv_utf8_invalid_utf8() {
        let data = b"key1 100\nkey2 200\n\xFF"; // Invalid UTF-8 byte
        let indices: HashSet<usize> = IndiceCache::new(&[0, 1]);
        let mut results = Vec::new();

        let result = parse_space_kv_utf8_cached_indices(data, &indices, |key, value| {
            results.push((key.to_string(), value));
        });

        assert!(result.is_err()); // Expect an error due to invalid UTF-8
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData); // Check for specific error kind
    }
}
