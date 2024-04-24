use std::{vec, path::{Path, PathBuf}, fs::{self, File}, str::FromStr, io::{Read, Seek}};

use crate::parsing_cgroupv2::CgroupV2Metric;

#[derive(Debug)]
pub struct CgroupV2MetricFile {
    pub name: String,
    pub path: PathBuf,
    pub file: File,
}

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


pub fn is_cgroups_v2() -> bool {
    let cgroup_fs_type = std::fs::metadata("/sys/fs/cgroup/").unwrap().file_type();
    if cgroup_fs_type.is_dir() {
        return true;
    } else {
        return false;
    }
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
            for entry in entries {
                if let Ok(entry_ok) = entry {
                    let mut path = entry_ok.path();
                    if path.is_dir(){
                        match path.file_name() {
                            Some(_dir_name) => {
                                let new_suffixe = if suffix.ends_with(".slice/") {
                                    &suffix[..suffix.len() - ".slice/".len()]
                                } else {
                                    suffix
                                };
                                match retrieve_name(&path, &new_suffixe.to_owned()){
                                    Some(name) => {
                                        path.push("cpu.stat");
                                        let file = match File::open(&path) {
                                            Err(why) => panic!("couldn't open {}: {}", path.display(), why),
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

pub fn list_all_k8s_pods_file() -> Vec<CgroupV2MetricFile>{
    let mut final_li_metric_file: Vec<CgroupV2MetricFile> = Vec::new();
    let root_directory_path: &str = "/sys/fs/cgroup/kubepods.slice/";
    if !Path::new(root_directory_path).exists() {
        println!("Le répertoire n'existe pas !");
        return final_li_metric_file
    }
    let all_sub_dir: Vec<String> = vec!["".to_string(), "kubepods-besteffort.slice/".to_string(), "kubepods-burstable.slice/".to_string()];
    for suffix in all_sub_dir{
        match list_metric_file_in_dir(&root_directory_path.to_owned(), &suffix.to_owned()){
            Ok(mut result_vec) => {
                final_li_metric_file.append(&mut result_vec);
            }
            Err(err) => {
                panic!("Can't append the two vectors because: {:?}", err);
            }
        }
    }
    return final_li_metric_file;
}


pub fn gather_value(file: &mut CgroupV2MetricFile) -> anyhow::Result<CgroupV2Metric>{
    // usage_usec : Le temps total d’utilisation du processeur par le groupe de processus, exprimé en microsecondes. Dans votre exemple, il s’élève à 54 566 400 122 microsecondes (soit environ 54,57 secondes).
    // user_usec : Le temps passé par les processus du groupe en mode utilisateur (c’est-à-dire lorsqu’ils exécutent du code applicatif), également en microsecondes. Dans votre cas, cela représente environ 35 190 757 954 microsecondes (environ 35,19 secondes).
    // system_usec : Le temps passé par les processus du groupe en mode noyau (lorsqu’ils exécutent des appels système ou des tâches de gestion du système), toujours en microsecondes. Dans votre exemple, cela équivaut à environ 19 375 642 167 microsecondes (environ 19,38 secondes).
    // nr_periods : Le nombre de périodes de contrôle (ou intervalles) pendant lesquelles le groupe de processus a été surveillé. Dans votre cas, il est de 0, ce qui signifie qu’aucune période de contrôle n’a été enregistrée.
    // nr_throttled : Le nombre de fois où le groupe de processus a été limité (throttled) en raison des contraintes imposées par le contrôleur CPU. Dans votre exemple, il est également de 0.
    // throttled_usec : Le temps total pendant lequel le groupe de processus a été limité (throttled), exprimé en microsecondes. Dans votre cas, il est de 0 microsecondes.
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