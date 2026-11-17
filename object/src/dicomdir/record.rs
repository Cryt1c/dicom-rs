//! Directory records contained in a DICOMDIR file.

use std::fmt;

use dicom_core::{DataDictionary, Tag};
use dicom_dictionary_std::tags;

use crate::{InMemDicomObject, StandardDataDictionary};

/// The type of a directory record.
///
/// This covers the most common record types and preserves any other value
/// as [`Other`](DirectoryRecordType::Other).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DirectoryRecordType {
    /// A patient record.
    Patient,
    /// A study record.
    Study,
    /// A series record.
    Series,
    /// An image record.
    Image,
    /// Any other record type.
    Other(String),
}

impl DirectoryRecordType {
    /// Parse a directory record type from its DICOM value.
    pub fn from_value(value: &str) -> Self {
        match value.trim_end_matches(|c: char| c.is_whitespace() || c == '\0') {
            "PATIENT" => Self::Patient,
            "STUDY" => Self::Study,
            "SERIES" => Self::Series,
            "IMAGE" => Self::Image,
            other => Self::Other(other.to_string()),
        }
    }

    /// Get the DICOM representation of this record type.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Patient => "PATIENT",
            Self::Study => "STUDY",
            Self::Series => "SERIES",
            Self::Image => "IMAGE",
            Self::Other(value) => value,
        }
    }
}

impl fmt::Display for DirectoryRecordType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A single record in the Directory Record Sequence of a DICOMDIR file.
///
/// Each record is itself a DICOM data set,
/// exposed through [`object`](DirectoryRecord::object).
#[derive(Debug, Clone)]
pub struct DirectoryRecord<D = StandardDataDictionary> {
    offset: u32,
    next_offset: Option<u32>,
    lower_offset: Option<u32>,
    record_type: DirectoryRecordType,
    referenced_file_id: Option<Vec<String>>,
    referenced_sop_class_uid: Option<String>,
    referenced_sop_instance_uid: Option<String>,
    referenced_transfer_syntax_uid: Option<String>,
    item: InMemDicomObject<D>,
}

impl<D> DirectoryRecord<D> {
    pub(crate) fn from_item(offset: u32, item: InMemDicomObject<D>) -> Self
    where
        D: DataDictionary + Clone,
    {
        let record_type = string_value(&item, tags::DIRECTORY_RECORD_TYPE)
            .map(|value| DirectoryRecordType::from_value(&value))
            .unwrap_or_else(|| DirectoryRecordType::Other(String::new()));

        Self {
            offset,
            next_offset: offset_value(&item, tags::OFFSET_OF_THE_NEXT_DIRECTORY_RECORD),
            lower_offset: offset_value(
                &item,
                tags::OFFSET_OF_REFERENCED_LOWER_LEVEL_DIRECTORY_ENTITY,
            ),
            record_type,
            referenced_file_id: multi_string_value(&item, tags::REFERENCED_FILE_ID),
            referenced_sop_class_uid: string_value(&item, tags::REFERENCED_SOP_CLASS_UID_IN_FILE),
            referenced_sop_instance_uid: string_value(
                &item,
                tags::REFERENCED_SOP_INSTANCE_UID_IN_FILE,
            ),
            referenced_transfer_syntax_uid: string_value(
                &item,
                tags::REFERENCED_TRANSFER_SYNTAX_UID_IN_FILE,
            ),
            item,
        }
    }

    /// The absolute byte offset of this record's item tag in the DICOMDIR file.
    pub fn offset(&self) -> u32 {
        self.offset
    }

    /// The offset of the next sibling record, or `None` at the end of a level.
    pub fn next_offset(&self) -> Option<u32> {
        self.next_offset
    }

    /// The offset of the first child record, or `None` if this is a leaf.
    pub fn lower_offset(&self) -> Option<u32> {
        self.lower_offset
    }

    /// The type of this directory record.
    pub fn record_type(&self) -> &DirectoryRecordType {
        &self.record_type
    }

    /// The referenced file ID components, if this record references a file.
    ///
    /// The components are returned separately because DICOM uses `\`
    /// as the separator, independently of the platform path separator.
    pub fn referenced_file_id(&self) -> Option<&[String]> {
        self.referenced_file_id.as_deref()
    }

    /// The referenced file ID as a single string using `\` separators.
    pub fn referenced_file_id_string(&self) -> Option<String> {
        self.referenced_file_id
            .as_ref()
            .map(|components| components.join("\\"))
    }

    /// The referenced SOP Class UID in the referenced file.
    pub fn referenced_sop_class_uid(&self) -> Option<&str> {
        self.referenced_sop_class_uid.as_deref()
    }

    /// The referenced SOP Instance UID in the referenced file.
    pub fn referenced_sop_instance_uid(&self) -> Option<&str> {
        self.referenced_sop_instance_uid.as_deref()
    }

    /// The referenced transfer syntax UID in the referenced file.
    pub fn referenced_transfer_syntax_uid(&self) -> Option<&str> {
        self.referenced_transfer_syntax_uid.as_deref()
    }

    /// The DICOM data set which makes up this directory record.
    pub fn object(&self) -> &InMemDicomObject<D> {
        &self.item
    }

    /// Whether this record has no lower-level records.
    pub fn is_leaf(&self) -> bool {
        self.lower_offset.is_none()
    }
}

fn trimmed(value: &str) -> &str {
    value.trim_end_matches(|c: char| c.is_whitespace() || c == '\0')
}

fn string_value<D>(item: &InMemDicomObject<D>, tag: Tag) -> Option<String>
where
    D: DataDictionary + Clone,
{
    item.element_opt(tag)
        .ok()
        .flatten()
        .and_then(|element| element.to_str().ok())
        .map(|value| trimmed(value.as_ref()).to_string())
        .filter(|value| !value.is_empty())
}

fn multi_string_value<D>(item: &InMemDicomObject<D>, tag: Tag) -> Option<Vec<String>>
where
    D: DataDictionary + Clone,
{
    let values = item
        .element_opt(tag)
        .ok()
        .flatten()
        .and_then(|element| element.to_multi_str().ok())?;

    let components = values
        .iter()
        .map(|value| trimmed(value).to_string())
        .collect::<Vec<_>>();

    if components.is_empty() || components.iter().all(String::is_empty) {
        None
    } else {
        Some(components)
    }
}

fn offset_value<D>(item: &InMemDicomObject<D>, tag: Tag) -> Option<u32>
where
    D: DataDictionary + Clone,
{
    item.element_opt(tag)
        .ok()
        .flatten()
        .and_then(|element| element.to_int::<u32>().ok())
        .filter(|offset| *offset != 0)
}
