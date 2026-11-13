use flops_memory::CpuInfo;
use std::io::Write;
use tempfile::NamedTempFile;

fn create_test_cpuinfo(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    file.write_all(content.as_bytes())
        .expect("Failed to write to temp file");
    file.flush().expect("Failed to flush temp file");
    file
}

#[test]
fn test_integration_read_x86_cpuinfo() {
    let content = "processor\t: 0\n\
                   vendor_id\t: GenuineIntel\n\
                   cpu family\t: 6\n\
                   model\t\t: 142\n\
                   stepping\t: 10\n\
                   microcode\t: 0xb4\n";

    let temp_file = create_test_cpuinfo(content);
    let result = CpuInfo::read(temp_file.path()).expect("Failed to read cpuinfo");

    assert_eq!(result.family, 6);
    assert_eq!(result.model, 142);
    assert_eq!(result.stepping, 10);
}

#[test]
fn test_integration_read_multi_processor() {
    let content = "processor\t: 0\n\
                   cpu family\t: 6\n\
                   model\t\t: 142\n\
                   stepping\t: 10\n\
                   \n\
                   processor\t: 1\n\
                   cpu family\t: 6\n\
                   model\t\t: 142\n\
                   stepping\t: 10\n";

    let temp_file = create_test_cpuinfo(content);
    let result = CpuInfo::read(temp_file.path()).expect("Failed to read cpuinfo");

    // Should only read from the first processor block
    assert_eq!(result.family, 6);
    assert_eq!(result.model, 142);
    assert_eq!(result.stepping, 10);
}

#[test]
fn test_integration_read_arm_hexadecimal() {
    let content = "processor\t: 0\n\
                   BogoMIPS\t: 38.40\n\
                   CPU implementer\t: 0x41\n\
                   CPU architecture: 0x8\n\
                   CPU variant\t: 0xc\n\
                   CPU part\t: 0xd08\n\
                   CPU revision\t: 2\n";

    let temp_file = create_test_cpuinfo(content);
    let result = CpuInfo::read(temp_file.path()).expect("Failed to read cpuinfo");

    assert_eq!(result.family, 0x8);
    assert_eq!(result.model, 0xd08);
}

#[test]
fn test_integration_missing_stepping() {
    let content = "processor\t: 0\n\
                   cpu family\t: 6\n\
                   model\t\t: 142\n";

    let temp_file = create_test_cpuinfo(content);
    let result = CpuInfo::read(temp_file.path()).expect("Failed to read cpuinfo");

    // stepping defaults to -1 if not present
    assert_eq!(result.family, 6);
    assert_eq!(result.model, 142);
    assert_eq!(result.stepping, -1);
}

#[test]
fn test_integration_various_whitespace() {
    let content = "processor: 0\n\
                   cpu family: 6\n\
                   model: 142\n\
                   stepping: 10\n";

    let temp_file = create_test_cpuinfo(content);
    let result = CpuInfo::read(temp_file.path()).expect("Failed to read cpuinfo");

    assert_eq!(result.family, 6);
    assert_eq!(result.model, 142);
    assert_eq!(result.stepping, 10);
}

#[test]
fn test_integration_arm_fallback() {
    let content = "processor\t: 0\n\
                   BogoMIPS\t: 38.40\n\
                   CPU architecture: 8\n\
                   CPU part\t: 0xd08\n";

    let temp_file = create_test_cpuinfo(content);
    let result = CpuInfo::read(temp_file.path()).expect("Failed to read cpuinfo");

    // Should use ARM fallback values
    assert_eq!(result.family, 8);
    assert_eq!(result.model, 0xd08);
}

#[test]
fn test_integration_x86_takes_precedence() {
    let content = "processor\t: 0\n\
                   cpu family\t: 6\n\
                   model\t\t: 142\n\
                   CPU architecture: 999\n\
                   CPU part\t: 0xfff\n";

    let temp_file = create_test_cpuinfo(content);
    let result = CpuInfo::read(temp_file.path()).expect("Failed to read cpuinfo");

    // x86 values should take precedence over ARM fallback
    assert_eq!(result.family, 6);
    assert_eq!(result.model, 142);
}

#[test]
fn test_integration_partial_arm_fallback() {
    let content = "processor\t: 0\n\
                   model\t\t: 142\n\
                   CPU architecture: 8\n\
                   CPU part\t: 0xd08\n";

    let temp_file = create_test_cpuinfo(content);
    let result = CpuInfo::read(temp_file.path()).expect("Failed to read cpuinfo");

    // Should use model from x86 and family from ARM
    assert_eq!(result.family, 8);
    assert_eq!(result.model, 142);
}

#[test]
fn test_integration_error_missing_both_family_and_model() {
    let content = "processor\t: 0\n\
                   stepping\t: 10\n\
                   flags\t\t: fpu\n";

    let temp_file = create_test_cpuinfo(content);
    let result = CpuInfo::read(temp_file.path());

    assert!(result.is_err());
}

#[test]
fn test_integration_error_invalid_integer() {
    let content = "processor\t: 0\n\
                   cpu family\t: not_a_number\n\
                   model\t\t: 142\n";

    let temp_file = create_test_cpuinfo(content);
    let result = CpuInfo::read(temp_file.path());

    assert!(result.is_err());
}

#[test]
fn test_integration_large_cpu_values() {
    let content = "processor\t: 0\n\
                   cpu family\t: 255\n\
                   model\t\t: 65535\n\
                   stepping\t: 15\n";

    let temp_file = create_test_cpuinfo(content);
    let result = CpuInfo::read(temp_file.path()).expect("Failed to read cpuinfo");

    assert_eq!(result.family, 255);
    assert_eq!(result.model, 65535);
    assert_eq!(result.stepping, 15);
}

#[test]
fn test_integration_empty_values() {
    let content = "processor\t: 0\n\
                   cpu family\t:\n\
                   model\t\t: 142\n";

    let temp_file = create_test_cpuinfo(content);
    let result = CpuInfo::read(temp_file.path());

    // Should fail because empty value can't be parsed
    assert!(result.is_err());
}

#[test]
fn test_integration_real_world_intel() {
    let content = "processor\t: 0\n\
                   vendor_id\t: GenuineIntel\n\
                   cpu family\t: 6\n\
                   model\t\t: 158\n\
                   model name\t: Intel(R) Core(TM) i7-7700K CPU @ 4.20GHz\n\
                   stepping\t: 9\n\
                   microcode\t: 0xb4\n\
                   cpu MHz\t\t: 4199.998\n\
                   cache size\t: 8192 KB\n\
                   physical id\t: 0\n\
                   siblings\t: 8\n\
                   core id\t\t: 0\n\
                   cpu cores\t: 4\n\
                   apicid\t\t: 0\n\
                   initial apicid\t: 0\n\
                   fpu\t\t: yes\n\
                   fpu_exception\t: yes\n\
                   cpuid level\t: 22\n\
                   wp\t\t: yes\n\
                   flags\t\t: fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 clflush dts acpi mmx fxsr sse sse2 ss ht tm pbe syscall nx pdpe1gb rdtscp lm constant_tsc arch_perfmon pebs bts rep_good nopl xtopology nonstop_tsc cpuid aperfmperf pni pclmulqdq dtes64 monitor ds_cpl vmx est tm2 ssse3 cx16 xtpr pdcm pcid sse4_1 sse4_2 x2apic popcnt tsc_deadline_timer aes xsave osxsave avx f16c rdrand lahf_lm abm cpuid_fault epb invpcid_single pti ssbd ibrs ibpb stibp tpr_shadow vnmi flexpriority ept vpid ept_ad fsgsbase tsc_adjust bmi1 hle avx2 smep bmi2 erms invpcid rtm mpx avx512f avx512dq rdseed adx smap avx512ifma clflushopt clwb avx512pf avx512er avx512cd sha_ni avx512bw avx512vl xsaveopt xsavec xgetbv1 xsaves dtherm arat pln pts hwp hwp_notify hwp_act_window hwp_epp hwp_pkg_req md_clear\n\
                   bugs\t\t: cpu_meltdown spectre_v1 spectre_v2\n\
                   bogomips\t: 8399.99\n\
                   clflush size\t: 64\n\
                   cache_alignment\t: 64\n\
                   address sizes\t: 39 bits physical, 48 bits virtual\n\
                   power management:\n";

    let temp_file = create_test_cpuinfo(content);
    let result = CpuInfo::read(temp_file.path()).expect("Failed to read cpuinfo");

    assert_eq!(result.family, 6);
    assert_eq!(result.model, 158);
    assert_eq!(result.stepping, 9);
}

#[test]
fn test_integration_real_world_arm() {
    let content = "processor\t: 0\n\
                   BogoMIPS\t: 38.40\n\
                   Features\t: half thumb fastmult vfp edsp neon vfpv4 idiva idivt vfpd32 lpae evtstrm crc32 v8 dcache_line_size\n\
                   CPU implementer\t: 0x41\n\
                   CPU architecture: 8\n\
                   CPU variant\t: 0xc\n\
                   CPU part\t: 0xd08\n\
                   CPU revision\t: 2\n\
                   \n\
                   processor\t: 1\n\
                   BogoMIPS\t: 38.40\n";

    let temp_file = create_test_cpuinfo(content);
    let result = CpuInfo::read(temp_file.path()).expect("Failed to read cpuinfo");

    assert_eq!(result.family, 8);
    assert_eq!(result.model, 0xd08);
}
