//! Errors which may occur while reading a DICOMDIR file.

use std::path::PathBuf;

use snafu::{Backtrace, Snafu};

use crate::{AccessError, AttributeError, ReadError};

/// An error which may occur while reading or traversing a DICOMDIR file.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum DicomDirError {
    /// Could not read the DICOMDIR file from disk.
    #[snafu(display("Could not read DICOMDIR '{}'", path.display()))]
    ReadDicomDir {
        path: PathBuf,
        source: std::io::Error,
        backtrace: Backtrace,
    },

    /// Could not parse the DICOMDIR file as a DICOM object.
    #[snafu(display("Could not parse DICOMDIR '{}'", path.display()))]
    OpenDicomDir {
        path: PathBuf,
        source: Box<ReadError>,
        backtrace: Backtrace,
    },

    /// Could not parse the file meta information of the DICOMDIR file.
    #[snafu(display("Could not parse DICOMDIR file meta information"))]
    ParseMetaDataSet {
        source: Box<crate::meta::Error>,
        backtrace: Backtrace,
    },

    /// The file is not a DICOMDIR file.
    #[snafu(display("Not a DICOMDIR (Media Storage SOP Class UID is '{uid}')"))]
    NotADicomDir { uid: String, backtrace: Backtrace },

    /// The file does not have the standard DICOM file header.
    #[snafu(display("Not a DICOM file: {reason}"))]
    InvalidDicomFile {
        reason: String,
        backtrace: Backtrace,
    },

    /// The Directory Record Sequence is absent.
    #[snafu(display("DICOMDIR is missing the Directory Record Sequence (0004,1220)"))]
    MissingDirectoryRecordSequence { backtrace: Backtrace },

    /// Could not access an element in the DICOMDIR data set.
    #[snafu(display("Could not access DICOMDIR element (0004,{element:04X})"))]
    AccessElement {
        element: u16,
        source: AccessError,
        backtrace: Backtrace,
    },

    /// The Directory Record Sequence is not a data set sequence.
    #[snafu(display("Directory Record Sequence (0004,1220) is not a data set sequence"))]
    InvalidDirectoryRecordSequence { backtrace: Backtrace },

    /// Could not read the Directory Record Sequence items.
    #[snafu(display("Could not read Directory Record Sequence items"))]
    ReadDirectoryRecordSequence {
        source: AttributeError,
        backtrace: Backtrace,
    },

    /// Could not create the data set reader used to locate item offsets.
    #[snafu(display("Could not create DICOMDIR data set reader"))]
    CreateDataSetReader {
        source: Box<dicom_parser::dataset::read::Error>,
        backtrace: Backtrace,
    },

    /// Could not scan the DICOMDIR data set for item offsets.
    #[snafu(display("Could not scan DICOMDIR data set"))]
    ScanDataSet {
        source: Box<dicom_parser::dataset::read::Error>,
        backtrace: Backtrace,
    },

    /// The DICOMDIR transfer syntax is not supported.
    #[snafu(display("Unsupported DICOMDIR transfer syntax '{uid}'"))]
    UnsupportedTransferSyntax { uid: String, backtrace: Backtrace },

    /// A directory record offset does not point to a known item.
    #[snafu(display("Directory record offset {offset} does not exist"))]
    MissingRecord { offset: u32, backtrace: Backtrace },

    /// A cycle was found while following directory record offsets.
    #[snafu(display("Cycle detected in directory record offsets at {offset}"))]
    OffsetCycle { offset: u32, backtrace: Backtrace },

    /// The number of collected offsets does not match the number of records.
    #[snafu(display("Expected {expected} directory record offsets, found {actual}"))]
    OffsetCountMismatch {
        expected: usize,
        actual: usize,
        backtrace: Backtrace,
    },

    /// A record which should reference a file does not have a file ID.
    #[snafu(display("Directory record at offset {offset} has no referenced file ID"))]
    MissingReferencedFileId { offset: u32, backtrace: Backtrace },

    /// A referenced file ID is not a safe relative path.
    #[snafu(display("Invalid referenced file ID {value:?}: {reason}"))]
    InvalidFileId {
        value: String,
        reason: String,
        backtrace: Backtrace,
    },

    /// Could not open a file referenced by a directory record.
    #[snafu(display("Could not open referenced file '{}'", path.display()))]
    OpenReferencedFile {
        path: PathBuf,
        source: Box<ReadError>,
        backtrace: Backtrace,
    },

    /// A referenced file does not match the metadata in its directory record.
    #[snafu(display(
        "Referenced file '{}' does not match the DICOMDIR record: {reason}",
        path.display()
    ))]
    ReferenceMismatch {
        path: PathBuf,
        reason: String,
        backtrace: Backtrace,
    },
}
