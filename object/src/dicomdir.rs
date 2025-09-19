use std::path::Path;

use dicom_dictionary_std::tags;

use crate::{FileDicomObject, InMemDicomObject, OpenFileOptions, ReadError};

pub struct DicomDir {
    file: FileDicomObject<InMemDicomObject>,
}

// DirectoryRecord {
// directory_record_type,
// referenced_file_id,
// referenced_sop_class_uid_in_file,
// referenced_sop_instance_uid_in_file,
// referenced_transfer_syntax_uid_in_file,
// referenced_transfer_syntax_uid_in_file,
// referenced_related_general_sop_class_uid_in_file,
// }

pub type Result<T, E = ReadError> = std::result::Result<T, E>;

impl DicomDir {
    pub fn new<P>(path: P) -> Result<DicomDir>
    where
        P: AsRef<Path>,
    {
        let default_dicom_object = OpenFileOptions::new()
            .open_file(path)
            .expect("could not load file");

        Ok(Self {
            file: default_dicom_object,
        })
    }
    pub fn get_directory_record_sequence(&self) -> Vec<InMemDicomObject> {
        let directory_record_sequence = self
            .file
            .element(tags::DIRECTORY_RECORD_SEQUENCE)
            .expect("could not get DIRECTORY_RECORD_SEQUENCE")
            .items()
            .expect("could not get items of DIRECTORY_RECORD_SEQUENCE");
        directory_record_sequence.to_vec()
    }

    pub fn get_referenced_file_ids(&self) -> Vec<String> {
        let directory_record_sequence = self
            .file
            .element(tags::DIRECTORY_RECORD_SEQUENCE)
            .expect("could not get DIRECTORY_RECORD_SEQUENCE")
            .items()
            .expect("could not get items of DIRECTORY_RECORD_SEQUENCE");

        let referenced_file_ids: Vec<_> = directory_record_sequence
            .iter()
            .filter_map(|item| {
                item.element(tags::REFERENCED_FILE_ID)
                    .ok()
                    .and_then(|element| element.to_str().ok())
                    .map(|cow_str| cow_str.into_owned())
            })
            .collect();
        referenced_file_ids
    }

    pub fn len(&self) -> usize {
        self.file
            .element(tags::DIRECTORY_RECORD_SEQUENCE)
            .expect("could not get DIRECTORY_RECORD_SEQUENCE")
            .items()
            .expect("could not get items of DIRECTORY_RECORD_SEQUENCE")
            .len()
    }
}

#[cfg(test)]
mod test {
    use super::DicomDir;
    use std::env;

    #[test]
    fn test_dicom_dir() {
        let current_dir = env::current_dir().unwrap();
        println!("Current directory: {}", current_dir.display());
        let path = "src/dicomdirtests/DICOMDIR";
        let dicom_dir_result = DicomDir::new(&path);
        let Ok(dicom_dir) = dicom_dir_result else {
            println!("could not load dicom_dir");
            return;
        };

        // println!("Referenced File IDs:");
        // let referenced_file_ids = dicom_dir.get_referenced_file_ids();
        // for file_id in &referenced_file_ids {
        //     println!("  {}", file_id);
        // }
        let directory_record_sequence = dicom_dir.get_directory_record_sequence();
        // for directory_record in &directory_record_sequence {
        //     println!("  {:?}", directory_record);
        // }

        println!("dicom_dir.len() {:?}", dicom_dir.len());
    }
}
