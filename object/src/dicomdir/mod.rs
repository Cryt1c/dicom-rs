//! Reading DICOMDIR files and the DICOM objects they reference.
//!
//! A DICOMDIR file is a DICOM file which describes a file-set.
//! Its `Directory Record Sequence` contains a flat sequence of directory
//! records, while the record offsets define the logical
//! patient/study/series/image hierarchy.
//!
//! This module provides:
//!
//! - [`DicomDir`], which parses the file and exposes its records.
//! - [`DirectoryRecord`], which wraps each directory record as a DICOM object.
//! - [`ReferencedFile`], which describes a DICOM file referenced by a record.
//!
//! # Example
//!
//! ```no_run
//! use dicom_object::dicomdir::DicomDir;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let dir = DicomDir::open("DICOMDIR")?;
//!
//! for record in dir.traverse() {
//!     println!("{:?} {:?}", record.record_type(), record.referenced_file_id());
//! }
//!
//! let objects = dir.open_all()?;
//! println!("loaded {} referenced DICOM objects", objects.len());
//! # Ok(())
//! # }
//! ```

mod error;
mod offsets;
mod record;

pub use error::DicomDirError;
pub use record::{DirectoryRecord, DirectoryRecordType};

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use dicom_core::{DataDictionary, Tag};
use dicom_dictionary_std::{tags, uids};

use crate::{
    DefaultDicomObject, FileDicomObject, FileMetaTable, InMemDicomObject, OpenFileOptions,
    StandardDataDictionary,
};

/// A parsed DICOMDIR file.
#[derive(Debug)]
pub struct DicomDir<D = StandardDataDictionary> {
    path: PathBuf,
    meta: FileMetaTable,
    file_set_id: Option<String>,
    first_offset: Option<u32>,
    last_offset: Option<u32>,
    records: Vec<DirectoryRecord<D>>,
    by_offset: HashMap<u32, usize>,
    traversal: Vec<usize>,
}

impl DicomDir<StandardDataDictionary> {
    /// Open and parse a DICOMDIR file.
    pub fn open<P>(path: P) -> Result<Self, DicomDirError>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref().to_path_buf();
        let bytes = fs::read(&path).map_err(|source| DicomDirError::ReadDicomDir {
            path: path.clone(),
            source,
            backtrace: snafu::Backtrace::capture(),
        })?;

        let file = OpenFileOptions::new().open_file(&path).map_err(|source| {
            DicomDirError::OpenDicomDir {
                path: path.clone(),
                source: Box::new(source),
                backtrace: snafu::Backtrace::capture(),
            }
        })?;

        let meta = file.meta().clone();
        let media_storage_sop_class_uid = meta.media_storage_sop_class_uid();
        if media_storage_sop_class_uid != uids::MEDIA_STORAGE_DIRECTORY_STORAGE {
            return Err(DicomDirError::NotADicomDir {
                uid: media_storage_sop_class_uid.to_string(),
                backtrace: snafu::Backtrace::capture(),
            });
        }

        let file_set_id = element_opt(&file, tags::FILE_SET_ID)?
            .and_then(|element| element.to_str().ok())
            .map(|value| trimmed(value.as_ref()).to_string())
            .filter(|value| !value.is_empty());

        let first_offset = root_offset(
            &file,
            tags::OFFSET_OF_THE_FIRST_DIRECTORY_RECORD_OF_THE_ROOT_DIRECTORY_ENTITY,
        )?;
        let last_offset = root_offset(
            &file,
            tags::OFFSET_OF_THE_LAST_DIRECTORY_RECORD_OF_THE_ROOT_DIRECTORY_ENTITY,
        )?;

        let sequence = element_opt(&file, tags::DIRECTORY_RECORD_SEQUENCE)?.ok_or_else(|| {
            DicomDirError::MissingDirectoryRecordSequence {
                backtrace: snafu::Backtrace::capture(),
            }
        })?;
        let items =
            sequence
                .items()
                .ok_or_else(|| DicomDirError::InvalidDirectoryRecordSequence {
                    backtrace: snafu::Backtrace::capture(),
                })?;

        let item_offsets = offsets::collect_directory_record_offsets(&bytes, &meta)?;
        if item_offsets.len() != items.len() {
            return Err(DicomDirError::OffsetCountMismatch {
                expected: items.len(),
                actual: item_offsets.len(),
                backtrace: snafu::Backtrace::capture(),
            });
        }

        let records = items
            .iter()
            .cloned()
            .zip(item_offsets)
            .map(|(item, offset)| DirectoryRecord::from_item(offset, item))
            .collect::<Vec<_>>();

        let mut by_offset = HashMap::with_capacity(records.len());
        for (index, record) in records.iter().enumerate() {
            by_offset.insert(record.offset(), index);
        }

        let traversal = build_traversal(&records, &by_offset, first_offset)?;

        Ok(Self {
            path,
            meta,
            file_set_id,
            first_offset,
            last_offset,
            records,
            by_offset,
            traversal,
        })
    }
}

impl<D> DicomDir<D> {
    /// The path of the DICOMDIR file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file meta information table of the DICOMDIR file.
    pub fn meta(&self) -> &FileMetaTable {
        &self.meta
    }

    /// The File-set ID, if present.
    pub fn file_set_id(&self) -> Option<&str> {
        self.file_set_id.as_deref()
    }

    /// The offset of the first root directory record, if any.
    pub fn first_offset(&self) -> Option<u32> {
        self.first_offset
    }

    /// The offset of the last root directory record, if any.
    pub fn last_offset(&self) -> Option<u32> {
        self.last_offset
    }

    /// All directory records, in file/sequence order.
    pub fn records(&self) -> &[DirectoryRecord<D>] {
        &self.records
    }

    /// Get a directory record by its absolute item offset.
    pub fn record(&self, offset: u32) -> Option<&DirectoryRecord<D>> {
        self.by_offset
            .get(&offset)
            .map(|index| &self.records[*index])
    }

    /// Traverse the directory hierarchy in logical order.
    ///
    /// The traversal starts at the first root record and visits each record
    /// before its children, following the `next` and `lower` offsets.
    /// Any cycles or dangling offsets are detected when the DICOMDIR is opened.
    pub fn traverse(&self) -> impl Iterator<Item = &DirectoryRecord<D>> + '_ {
        self.traversal.iter().map(|index| &self.records[*index])
    }

    /// Iterate over the root-level directory records.
    pub fn root_records(&self) -> impl Iterator<Item = &DirectoryRecord<D>> + '_ {
        std::iter::successors(
            self.first_offset.and_then(|offset| self.record(offset)),
            |record| record.next_offset().and_then(|offset| self.record(offset)),
        )
    }

    /// Iterate over directory records which reference a DICOM file.
    ///
    /// The path of each referenced file is resolved relative to the
    /// directory containing the DICOMDIR file.
    pub fn referenced_files(
        &self,
    ) -> impl Iterator<Item = Result<ReferencedFile<'_, D>, DicomDirError>> + '_ {
        self.records.iter().filter_map(|record| {
            record.referenced_file_id().map(|components| {
                resolve_file_id(self.root_directory(), components)
                    .map(|path| ReferencedFile { record, path })
            })
        })
    }

    fn root_directory(&self) -> &Path {
        self.path.parent().unwrap_or_else(|| Path::new(""))
    }
}

impl<D> DicomDir<D>
where
    D: DataDictionary + Clone + Default,
{
    /// Open the DICOM file referenced by a directory record.
    pub fn open_referenced_file(
        &self,
        record: &DirectoryRecord<D>,
    ) -> Result<DefaultDicomObject<D>, DicomDirError> {
        let components =
            record
                .referenced_file_id()
                .ok_or_else(|| DicomDirError::MissingReferencedFileId {
                    offset: record.offset(),
                    backtrace: snafu::Backtrace::capture(),
                })?;

        let path = resolve_file_id(self.root_directory(), components)?;
        let object = OpenFileOptions::<D>::default()
            .open_file(&path)
            .map_err(|source| DicomDirError::OpenReferencedFile {
                path: path.clone(),
                source: Box::new(source),
                backtrace: snafu::Backtrace::capture(),
            })?;

        if let Some(expected) = record.referenced_sop_class_uid() {
            let actual = object.meta().media_storage_sop_class_uid();
            if actual != expected {
                return Err(DicomDirError::ReferenceMismatch {
                    path,
                    reason: format!(
                        "referenced SOP Class UID is {expected:?}, but the file has {actual:?}"
                    ),
                    backtrace: snafu::Backtrace::capture(),
                });
            }
        }

        if let Some(expected) = record.referenced_sop_instance_uid() {
            let actual = object.meta().media_storage_sop_instance_uid();
            if actual != expected {
                return Err(DicomDirError::ReferenceMismatch {
                    path,
                    reason: format!(
                        "referenced SOP Instance UID is {expected:?}, but the file has {actual:?}"
                    ),
                    backtrace: snafu::Backtrace::capture(),
                });
            }
        }

        Ok(object)
    }

    /// Iterate over the DICOM objects referenced by this DICOMDIR.
    ///
    /// Files are opened lazily, one at a time.
    pub fn open_referenced_files(
        &self,
    ) -> impl Iterator<Item = Result<DefaultDicomObject<D>, DicomDirError>> + '_ {
        self.records
            .iter()
            .filter(|record| record.referenced_file_id().is_some())
            .map(|record| self.open_referenced_file(record))
    }

    /// Open every DICOM object referenced by this DICOMDIR.
    ///
    /// This is a convenience method which collects all referenced objects.
    /// Use [`open_referenced_files`](Self::open_referenced_files) to load
    /// them lazily instead.
    pub fn open_all(&self) -> Result<Vec<DefaultDicomObject<D>>, DicomDirError> {
        self.open_referenced_files().collect()
    }
}

/// A DICOM file referenced by a directory record.
#[derive(Debug)]
pub struct ReferencedFile<'a, D = StandardDataDictionary> {
    record: &'a DirectoryRecord<D>,
    path: PathBuf,
}

impl<'a, D> ReferencedFile<'a, D> {
    /// The directory record which references this file.
    pub fn record(&self) -> &'a DirectoryRecord<D> {
        self.record
    }

    /// The resolved path of the referenced file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn element_opt<D>(
    file: &FileDicomObject<InMemDicomObject<D>>,
    tag: Tag,
) -> Result<Option<&crate::mem::InMemElement<D>>, DicomDirError>
where
    D: DataDictionary + Clone,
{
    file.element_opt(tag)
        .map_err(|source| DicomDirError::AccessElement {
            element: tag.element(),
            source,
            backtrace: snafu::Backtrace::capture(),
        })
}

fn root_offset<D>(
    file: &FileDicomObject<InMemDicomObject<D>>,
    tag: Tag,
) -> Result<Option<u32>, DicomDirError>
where
    D: DataDictionary + Clone,
{
    Ok(element_opt(file, tag)?
        .and_then(|element| element.to_int::<u32>().ok())
        .filter(|offset| *offset != 0))
}

fn build_traversal<D>(
    records: &[DirectoryRecord<D>],
    by_offset: &HashMap<u32, usize>,
    first_offset: Option<u32>,
) -> Result<Vec<usize>, DicomDirError> {
    let Some(first_offset) = first_offset else {
        return Ok(Vec::new());
    };

    let mut traversal = Vec::with_capacity(records.len());
    let mut visited = HashSet::with_capacity(records.len());
    let mut stack = vec![first_offset];

    while let Some(offset) = stack.pop() {
        if !visited.insert(offset) {
            return Err(DicomDirError::OffsetCycle {
                offset,
                backtrace: snafu::Backtrace::capture(),
            });
        }

        let index = *by_offset
            .get(&offset)
            .ok_or_else(|| DicomDirError::MissingRecord {
                offset,
                backtrace: snafu::Backtrace::capture(),
            })?;

        traversal.push(index);
        let record = &records[index];

        // Push next first so children (lower) are visited before siblings.
        if let Some(next_offset) = record.next_offset() {
            stack.push(next_offset);
        }
        if let Some(lower_offset) = record.lower_offset() {
            stack.push(lower_offset);
        }
    }

    Ok(traversal)
}

fn resolve_file_id(root: &Path, components: &[String]) -> Result<PathBuf, DicomDirError> {
    if components.is_empty() {
        return Err(invalid_file_id(components, "file ID has no components"));
    }

    let mut path = root.to_path_buf();
    for component in components {
        if component.is_empty() {
            return Err(invalid_file_id(
                components,
                "file ID contains an empty component",
            ));
        }
        if component == "." || component == ".." {
            return Err(invalid_file_id(
                components,
                "file ID contains a relative path component",
            ));
        }

        let mut parts = Path::new(component).components();
        match (parts.next(), parts.next()) {
            (Some(Component::Normal(_)), None) => path.push(component),
            _ => {
                return Err(invalid_file_id(
                    components,
                    "file ID contains an absolute or multi-component path",
                ));
            }
        }
    }

    Ok(path)
}

fn invalid_file_id(components: &[String], reason: &str) -> DicomDirError {
    DicomDirError::InvalidFileId {
        value: components.join("\\"),
        reason: reason.to_string(),
        backtrace: snafu::Backtrace::capture(),
    }
}

fn trimmed(value: &str) -> &str {
    value.trim_end_matches(|c: char| c.is_whitespace() || c == '\0')
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn fixture(name: &str) -> String {
        format!("src/dicomdirtests/{name}")
    }

    #[test]
    fn reads_expected_records_and_references() {
        let dir = DicomDir::open(fixture("DICOMDIR")).unwrap();

        assert_eq!(dir.records().len(), 52);
        assert_eq!(dir.referenced_files().count(), 31);
    }

    #[test]
    fn opens_all_referenced_objects() {
        let dir = DicomDir::open(fixture("DICOMDIR")).unwrap();
        let objects = dir.open_all().unwrap();

        assert_eq!(objects.len(), 31);
        for (file, object) in dir
            .referenced_files()
            .map(Result::unwrap)
            .zip(objects.iter())
        {
            if let Some(expected) = file.record().referenced_sop_instance_uid() {
                assert_eq!(
                    object.meta().media_storage_sop_instance_uid(),
                    expected,
                    "SOP Instance UID mismatch for {}",
                    file.path().display()
                );
            }
        }
    }

    #[test]
    fn recovers_absolute_item_offsets() {
        let dir = DicomDir::open(fixture("DICOMDIR")).unwrap();

        assert_eq!(dir.records()[0].offset(), 396);
        assert_eq!(dir.records()[1].offset(), 510);
        assert_eq!(dir.records()[2].offset(), 724);
        assert_eq!(dir.record(396).unwrap().referenced_file_id(), None);
        assert_eq!(
            dir.record(856).unwrap().referenced_file_id(),
            Some(["77654033", "CR1", "6154"].map(String::from).as_slice())
        );
    }

    #[test]
    fn traverses_hierarchy_in_logical_order() {
        let dir = DicomDir::open(fixture("DICOMDIR")).unwrap();

        let types = dir
            .traverse()
            .map(|record| record.record_type().as_str())
            .collect::<Vec<_>>();

        assert_eq!(types.len(), 52);
        assert_eq!(types.first().copied(), Some("PATIENT"));
        assert_eq!(dir.root_records().count(), 2);
    }

    #[test]
    fn supports_implicit_and_big_endian_transfer_syntaxes() {
        for name in ["DICOMDIR-implicit", "DICOMDIR-bigEnd"] {
            let dir = DicomDir::open(fixture(name)).unwrap();
            assert_eq!(dir.records().len(), 52, "{name}");
            assert_eq!(dir.referenced_files().count(), 31, "{name}");
            assert_eq!(dir.open_all().unwrap().len(), 31, "{name}");
        }
    }

    #[test]
    fn reordered_sequence_has_the_same_tree() {
        let base = DicomDir::open(fixture("DICOMDIR")).unwrap();
        let reordered = DicomDir::open(fixture("DICOMDIR-reordered")).unwrap();

        let base_types = base
            .traverse()
            .map(|record| record.record_type().as_str())
            .collect::<Vec<_>>();
        let reordered_types = reordered
            .traverse()
            .map(|record| record.record_type().as_str())
            .collect::<Vec<_>>();

        assert_eq!(base_types, reordered_types);
    }

    #[test]
    fn handles_empty_file_set() {
        let dir = DicomDir::open(fixture("DICOMDIR-empty.dcm")).unwrap();

        assert_eq!(dir.records().len(), 0);
        assert_eq!(dir.referenced_files().count(), 0);
        assert_eq!(dir.first_offset(), None);
        assert_eq!(dir.last_offset(), None);
        assert_eq!(dir.traverse().count(), 0);
    }

    #[test]
    fn missing_offsets_return_a_clear_error() {
        // This fixture is intentionally corrupted: the removed zero-offset
        // elements were not compensated for in the final item length.
        let result = DicomDir::open(fixture("DICOMDIR-nooffset"));

        assert!(matches!(result, Err(DicomDirError::OpenDicomDir { .. })));
    }

    #[test]
    fn preserves_unknown_record_types() {
        let dir = DicomDir::open(fixture("DICOMDIR-nopatient")).unwrap();

        assert!(
            dir.records()
                .iter()
                .any(|record| matches!(record.record_type(), DirectoryRecordType::Other(_)))
        );
    }

    #[test]
    fn rejects_path_traversal() {
        let error = resolve_file_id(Path::new("root"), &["..".to_string()]).unwrap_err();
        assert!(matches!(error, DicomDirError::InvalidFileId { .. }));

        let error = resolve_file_id(Path::new("root"), &["a/b".to_string()]).unwrap_err();
        assert!(matches!(error, DicomDirError::InvalidFileId { .. }));
    }

    #[test]
    fn resolves_file_ids_relative_to_dicomdir() {
        let path = resolve_file_id(
            Path::new("a/b"),
            &[
                "PATIENT".to_string(),
                "STUDY".to_string(),
                "IMAGE".to_string(),
            ],
        )
        .unwrap();

        assert_eq!(path, Path::new("a/b/PATIENT/STUDY/IMAGE"));
    }
}
