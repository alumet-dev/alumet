use std::{vec, path::{Path, PathBuf}, fs::{self, File}, str::FromStr, io::{Read, Seek}};

use anyhow::Error;

use crate::parsing_cgroupv2::CgroupV2Metric;

/// CgroupV2MetricFile represente a file containing cgroup v2 data about cpu usage
/// We use:
/// - the pod's name the file is about
/// - the path to the file
/// - a File to simplify reading of values

#[derive(Debug)]
pub struct CgroupV2MetricFile {
    pub name: String,
    pub path: PathBuf,
    pub file: File,
}

/// Create a new CgroupV2MetricFile structure from a name, a path and a File
impl CgroupV2MetricFile{
    fn new(name: String, path_entry: PathBuf, file: File) -> CgroupV2MetricFile{
        CgroupV2MetricFile{
            name: name,
            path: path_entry,
            file: file,
        }
    }
}

impl IntoIterator for CgroupV2MetricFile {
    type Item = PathBuf;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        vec![self.path].into_iter()
    }
}

/// Check if a specific file is a dir. Used to know if cgroupv2 are used
pub fn is_accessible_dir(path: PathBuf) -> bool {
    if let Ok(metadata) = std::fs::metadata(&path) {
        metadata.is_dir()
    } else {
        false
    }
}

/// Knowing a prefix to remove in a name, retrieve it. Adapted to Cgroup naming convention
/// Exemple: 
/// path is: "myPath/kubepods-burstable.slice/kubepods-burstable-podABCD"
/// prefix should be "kubepods-burstable"
/// Return should be podABCD
/// NB: ".slice" part was removed earlier
fn retrieve_name(path: &Path, prefix: &String) -> Option<String> {
    // Get the last component of the path (file or directory name)
    if prefix != ""{
        if let Some(file_name) = path.file_name() {
            if let Some(name) = file_name.to_str() {
                let begin = prefix.len();
                let without_prefix = if name.starts_with(prefix) {
                    &name[begin+1..]
                } else {
                    name
                };
                let without_suffix = if without_prefix.ends_with(".slice") {
                    &without_prefix[..without_prefix.len() - ".slice".len()]
                } else {
                    without_prefix
                };
                return Some(without_suffix.to_owned());
            } else {
                log::debug!("Invalid UTF-8 in file name");
                return None;
            }
        } else {
            log::debug!("No file or directory name found")
        }
    }
    None
}

/// Return a Result containing an error or a Vector of CgroupV2MetricFile 
/// associated to pods availables under a directory (root_directory_path)
fn list_metric_file_in_dir(root_directory_path: &String, prefix: &String) -> anyhow::Result<Vec<CgroupV2MetricFile>> {
    let full_path = format!("{}{}", root_directory_path, prefix);
    let dir = Path::new(&full_path);
    let mut vec_file_metric: Vec<CgroupV2MetricFile> = Vec::new();
    match fs::read_dir(dir){
        Ok(entries) => {
            for entry in entries {
                if let Ok(entry_ok) = entry {
                    let mut path = entry_ok.path();
                    if path.is_dir(){
                        let _dir_name = path.file_name().expect("Impossible to write dir name");
                        let new_prefixe = if prefix.ends_with(".slice/") {
                            &prefix[..prefix.len() - ".slice/".len()]
                        } else {
                            prefix
                        };
                        match retrieve_name(&path, &new_prefixe.to_owned()){
                            Some(name) => {
                                path.push("cpu.stat");
                                let file = match File::open(&path) {
                                    Err(why) => {
                                        return {
                                            let message = format!("couldn't open {}: {}", path.display(), why);
                                            Err(anyhow::Error::msg(message))
                                        }
                                    },
                                    Ok(file) => file,
                                };
                                vec_file_metric.push(CgroupV2MetricFile{
                                    name: name, 
                                    path: path,
                                    file: file,
                                });
                            }
                            None => {
                                continue
                            }
                        }
                    }
                }
            }
            return Ok(vec_file_metric);
        },
        Err(err) => {
            log::error!("Erreur lors de la lecture du répertoire : {err}");
            return Err(Error::from(err));
        }
    }
}

/// This function list all k8s pods availables, using 3 sub-directory to look in:
/// For each subdirectory, we look in if there is a directory/ies about pods and we add it
/// to a vector. All subdirectory are visited with the help of <list_metric_file_in_dir> function.
pub fn list_all_k8s_pods_file() -> anyhow::Result<Vec<CgroupV2MetricFile>>{
    let mut final_li_metric_file: Vec<CgroupV2MetricFile> = Vec::new();
    let root_directory_path: &str = "/sys/fs/cgroup/kubepods.slice/";
    if !Path::new(root_directory_path).exists() {
        println!("Le répertoire n'existe pas !");
        return Ok(final_li_metric_file);
    }
    let all_sub_dir: Vec<String> = vec!["".to_string(), "kubepods-besteffort.slice/".to_string(), "kubepods-burstable.slice/".to_string()];
    for prefix in all_sub_dir{
        let mut result_vec = list_metric_file_in_dir(&root_directory_path.to_owned(), &prefix.to_owned())?;
        final_li_metric_file.append(&mut result_vec);
    }
    return Ok(final_li_metric_file);
}

/// Giving as an argument a CgroupV2MetricFile this function retrieve a Result containing an 
/// error or a CgroupV2Metric containing all we need
pub fn gather_value(file: &mut CgroupV2MetricFile) -> anyhow::Result<CgroupV2Metric>{
    let mut content_file = String::new();
    file.file.read_to_string(&mut content_file).expect("Unable to read the file gathering values");
    file.file.rewind()?;
    match CgroupV2Metric::from_str(&content_file) {
        Ok(mut new_met) => {
            new_met.name = file.name.clone();
            return Ok(new_met);
        }
        Err(err) => {
            anyhow::bail!(format!("cgroupv2 test failed to parse for #{content_file} --- {:?}", err));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_cgroups_v2() {
        let tmp = std::env::temp_dir();
        let root: std::path::PathBuf = tmp.join("test-alumet-plugin-k8s/is_cgroupv2");
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        let cgroupv2_dir = root.join("myDirCgroup");
        std::fs::create_dir_all(&cgroupv2_dir).unwrap();
        assert!(is_accessible_dir(cgroupv2_dir));
        assert!(!is_accessible_dir(std::path::PathBuf::from("test-alumet-plugin-k8s/is_cgroupv2/myDirCgroup_bad")));
    }

    #[test]
    fn test_retrieve_name(){
        let tmp = std::env::temp_dir();
        let root: std::path::PathBuf = tmp.join("test-alumet-plugin-k8s/kubepods-besteffort.slice");
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        let burstable_dir = root.join("kubepods-burstable.slice");
        let besteffort_dir = root.join("kubepods-besteffort.slice");
        std::fs::create_dir_all(&burstable_dir).unwrap();
        std::fs::create_dir_all(&besteffort_dir).unwrap();

        let a = burstable_dir.join("kubepods-burstable-pod32a1942cb9a81912549c152a49b5f9b1.slice");
        let b = burstable_dir.join("kubepods-besteffort-podd9209de2b4b526361248c9dcf3e702c0.slice");
        let c = besteffort_dir.join("kubepods-besteffort-pod32a1942cb9a81912549c152a49b5f9b1.slice");
        let d = besteffort_dir.join("kubepods-burstable-podd9209de2b4b526361248c9dcf3e702c0.slice");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::create_dir_all(&c).unwrap();
        std::fs::create_dir_all(&d).unwrap();
        match retrieve_name(&a, &"kubepods-burstable".to_string()){
            Some(name_returned) => {
                assert_eq!(name_returned, "pod32a1942cb9a81912549c152a49b5f9b1");
            }
            None => {
                assert!(false);
            }
        };
        match retrieve_name(&b, &"kubepods-burstable".to_string()){
            Some(name_returned) => {
                assert_eq!(name_returned, "kubepods-besteffort-podd9209de2b4b526361248c9dcf3e702c0");
            }
            None => {
                assert!(false);
            }
        };
        match retrieve_name(&c, &"kubepods-besteffort".to_string()){
            Some(name_returned) => {
                assert_eq!(name_returned, "pod32a1942cb9a81912549c152a49b5f9b1")
            }
            None => {
                assert!(false);
            }
        };
        match retrieve_name(&d, &"kubepods-besteffort".to_string()){
            Some(name_returned) => {
                assert_eq!(name_returned, "kubepods-burstable-podd9209de2b4b526361248c9dcf3e702c0");
            }
            None => {
                assert!(false);
            }
        };
        let path_buf = PathBuf::from("");
        let name = "zkjbf".to_string();
        match retrieve_name(path_buf.as_path(), &name){
            Some(_name_returned) => {
                assert!(false)
            }
            None => {
                assert!(true);
            }
        };

    
    
    }
    #[test]
    fn test_list_metric_file_in_dir(){
        let tmp = std::env::temp_dir();
        let root: std::path::PathBuf = tmp.join("test-alumet-plugin-k8s/kubepods-folder.slice/");
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }                         
        let burstable_dir = root.join("kubepods-burstable.slice/");
        std::fs::create_dir_all(&burstable_dir).unwrap();

        let a = burstable_dir.join("kubepods-burstable-pod32a1942cb9a81912549c152a49b5f9b1.slice/");
        let b = burstable_dir.join("kubepods-burstable-podd9209de2b4b526361248c9dcf3e702c0.slice/");
        let c = burstable_dir.join("kubepods-burstable-podccq5da1942a81912549c152a49b5f9b1.slice/");
        let d = burstable_dir.join("kubepods-burstable-podd87dz3z8z09de2b4b526361248c902c0.slice/");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::create_dir_all(&c).unwrap();
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(a.join("cpu.stat"), "en").unwrap();
        std::fs::write(b.join("cpu.stat"), "fr").unwrap();
        std::fs::write(c.join("cpu.stat"), "sv").unwrap();
        std::fs::write(d.join("cpu.stat"), "ne").unwrap();
        let li_met_file: anyhow::Result<Vec<CgroupV2MetricFile>> =  list_metric_file_in_dir(&root.into_os_string().into_string().unwrap(), &"kubepods-burstable.slice/".to_owned());
        // let unwrap_li = li_met_file.with_context(|| {format!("VEC in test"); assert!(false);}));
        let list_pod_name = [
            "pod32a1942cb9a81912549c152a49b5f9b1",
            "podd9209de2b4b526361248c9dcf3e702c0",
            "podccq5da1942a81912549c152a49b5f9b1",
            "podd87dz3z8z09de2b4b526361248c902c0",
        ];

        match li_met_file {
            Ok(unwrap_li) => {
                assert_eq!(unwrap_li.len(), 4);
                for pod in unwrap_li {
                    if !list_pod_name.contains(&pod.name.as_str()) {
                        log::error!("Pod name not in the list: {}",pod.name);
                        assert!(false);
                    }
                }
            }
            Err(err) => {
                log::error!("Error reading li_met_file: {:?}", err);
                assert!(false);
            }
        }
        assert!(true);
    
    }
    #[test]
    fn test_gather_value(){
        let tmp = std::env::temp_dir();
        let root: std::path::PathBuf = tmp.join("test-alumet-plugin-k8s/kubepods-gather.slice/");
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }                         
        let burstable_dir = root.join("kubepods-burstable.slice/");
        std::fs::create_dir_all(&burstable_dir).unwrap();

        let a = burstable_dir.join("kubepods-burstable-pod32a1942cb9a81912549c152a49b5f9b1.slice/");
       
        std::fs::create_dir_all(&a).unwrap();
        let path_file = a.join("cpu.stat"); 
        std::fs::write( path_file.clone(), format!("usage_usec 8335557927\n
                                                        user_usec 4728882396\n
                                                        system_usec 3606675531\n
                                                        nr_periods 0\n
                                                        nr_throttled 0\n
                                                        throttled_usec 0")).unwrap();

        let file = match File::open(&path_file) {
            Err(why) => panic!("couldn't open {}: {}", path_file.display(), why),
            Ok(file) => file,
        };

        let mut my_cgroup_test_file: CgroupV2MetricFile = CgroupV2MetricFile::new("testing_pod".to_string(), path_file, file);



        let res_metric = gather_value(&mut my_cgroup_test_file);
        if let Ok(CgroupV2Metric { name, time_used_tot, time_used_user_mode, time_used_system_mode }) = res_metric{
            assert_eq!(name, "testing_pod".to_owned());
            assert_eq!(time_used_tot, 8335557927);
            assert_eq!(time_used_user_mode, 4728882396);
            assert_eq!(time_used_system_mode, 3606675531);
        }else{
            assert!(false);
        }
    }
    
}