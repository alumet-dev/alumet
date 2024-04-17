use std::{collections::HashMap, vec, path::Path};
use std::fs::{self};
use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, PartialEq)]
pub struct PrometheusMetric {
    pub name: String,
    pub labels: HashMap<String, String>,
    pub value: f64,
    pub timestamp: Option<u64>,
}

#[derive(Debug, PartialEq)]
pub struct CgroupV2MetricFile {
    pub name: String,
    pub path: PathBuf,
}

impl CgroupV2MetricFile{
    fn new(name: String, path_entry: PathBuf) -> CgroupV2MetricFile{
        CgroupV2MetricFile{
            name: name,
            path: path_entry
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


pub fn is_cgroups_v2() -> bool {
    let cgroup_fs_type = std::fs::metadata("/sys/fs/cgroup/").unwrap().file_type();
    if cgroup_fs_type.is_dir() {
        return true;
    } else {
        return false;
    }
}

fn rec_visit_dir(dir: &Path) -> anyhow::Result<()> {
    if !dir.is_dir(){
        match dir.file_name() {
            Some(file_name) => {
                println!("\tFILE - Nom du fichier : {:?}", file_name);        
            }
            None => {
                panic!("Impossible to write file name");
            }  
        }
        return Ok(());
    }else{
        match fs::read_dir(dir){
            Ok(entries) => {
                for entry in entries {
                    if let Ok(entry_ok) = entry {
                        let path = entry_ok.path();
                        match path.file_name() {
                            Some(dir_name) => {
                                println!("DIR - Nom du dossier : {:?}", dir_name);
                            }
                            None => {
                                panic!("Impossible to write dir name");
                            }  
                        }
                        rec_visit_dir(&path)?;
                    }
                }
                return Ok(());
            }
            Err(err) => {
                eprintln!("Erreur lors de la lecture du répertoire : {}", err);
                return Err(anyhow::Error::from(err));
            }
        }
    }
    return Ok(());
}

fn retrieve_name(path: &Path, prefix: &String) -> Option<String> {
    // Get the last component of the path (file or directory name)
    if prefix != ""{
        if let Some(file_name) = path.file_name() {
            if let Some(name) = file_name.to_str() {
                // println!("Extracted part: {}, prefix is: {:?}", name, prefix);
                let begin = prefix.len();
                let without_prefix = if name.starts_with(prefix) {
                    &name[begin+1..]
                } else {
                    name
                };
                // println!("Intermediaire: {:?} len=: {:?}", without_prefix, begin);
                let without_suffix = if without_prefix.ends_with(".slice") {
                    &without_prefix[..without_prefix.len() - ".slice".len()]
                } else {
                    without_prefix
                };
                // println!("\t\tRETURN = {:?}", without_suffix);
                return Some(without_suffix.to_owned());
            } else {
                println!("Invalid UTF-8 in file name");
                return None;
            }
        } else {
            println!("No file or directory name found");
        }
    }
    return None;
}

fn list_metric_file_in_dir(root_directory_path: &String, suffix: &String) -> anyhow::Result<Vec<CgroupV2MetricFile>> {
    let full_path = format!("{}{}", root_directory_path, suffix);
    let dir = Path::new(&full_path);
    let mut vec_file_metric: Vec<CgroupV2MetricFile> = Vec::new();
    match fs::read_dir(dir){
        Ok(entries) => {
            // println!("Entry for {:?}:",dir);
            for entry in entries {
                if let Ok(entry_ok) = entry {
                    let mut path = entry_ok.path();
                    if path.is_dir(){
                        match path.file_name() {
                            Some(dir_name) => {
                                // println!("\tDIR - Nom du dossier : {:?}", dir_name);
                                // let new_suffixe = &suffix[..suffix.len()-"slice/".len()];
                                let new_suffixe = if suffix.ends_with(".slice/") {
                                    &suffix[..suffix.len() - ".slice/".len()]
                                } else {
                                    suffix
                                };
                                match retrieve_name(&path, &new_suffixe.to_owned()){
                                    Some(name) => {
                                        path.push("cpu.stat");
                                        // println!("Name is: {:?} path is: {:?}",name,path);
                                        vec_file_metric.push(CgroupV2MetricFile{
                                            name: name, 
                                            path: path,
                                        });
                                    }
                                    None => {
                                        continue
                                    }
                                }
                            }
                            None => {
                                panic!("Impossible to write dir name");
                            }  
                        }
                    }
                }
            }
            return Ok(vec_file_metric);
        }
        Err(err) => {
            eprintln!("Erreur lors de la lecture du répertoire : {}", err);
            return Err(anyhow::Error::from(err));
        }
    }
}

pub fn list_all_K8S_pods() -> Vec<String>{
    let all_pods: Vec<String> = Vec::new();
    let root_directory_path: &str = "/sys/fs/cgroup/kubepods.slice/";
    if !Path::new(root_directory_path).exists() {
        println!("Le répertoire n'existe pas !");
        return all_pods
    }
    let all_sub_dir: Vec<String> = vec!["".to_string(), "kubepods-besteffort.slice/".to_string(), "kubepods-burstable.slice/".to_string()];
    //Look for pod in the root directory path
    // match fs::read_dir(root_directory_path) {
    //     Ok(entries) => {
    //         for entry in entries {
    //             if let Ok(entry) = entry {
    //                 let element_name = entry.file_name();
    //                 if entry.path().is_dir(){
    //                     println!("DIR - Nom du dossier : {:?}", element_name);
    //                 }else{
    //                     println!("FILE - Nom du fichier : {:?}", element_name);
    //                 }
                    
    //             }
    //         }
    //     }
    //     Err(err) => {
    //         eprintln!("Erreur lors de la lecture du répertoire : {}", err);
    //     }
    // }
    // visit_dir(Path::new(root_directory_path));
    let mut final_li_metric_file: Vec<CgroupV2MetricFile> = Vec::new();
    for suffix in all_sub_dir{
        match list_metric_file_in_dir(&root_directory_path.to_owned(), &suffix.to_owned()){
            Ok(mut result_vec) => {
                final_li_metric_file.append(&mut result_vec);
            }
            Err(err) => {
                panic!("Can't append the two vectors");
            }
        }
        // let mut tmp_vec: Vec<CgroupV2MetricFile> = list_metric_file_in_dir(&root_directory_path.to_owned(), &suffix.to_owned());
        
    }
    for elem in final_li_metric_file{
        println!("{:?}", elem);
    }

    return all_pods;
}

pub fn gather_value(all_files: Vec<CgroupV2MetricFile>) {



}