use std::ffi::OsString;
use std::sync::RwLock;

use log::{debug, info, warn};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::*;
use objc2_app_kit::*;
use objc2_foundation::*;

lazy_static::lazy_static! {
    static ref PASTEBOARD_LOCK: RwLock<()> = RwLock::new(());
}

#[derive(Debug)]
pub enum Error {
    UnsupportedType,
}

#[derive(Debug)]
pub enum Item {
    File(Retained<NSData>, OsString),
    Text(Retained<NSString>),
    FileUrl(Retained<NSString>),
    Unsupported(),
}

impl Item {
    fn get_extension(value: &NSPasteboardType) -> OsString {
        unsafe {
            if value.isEqualToString(NSPasteboardTypePDF) {
                OsString::from("output.pdf")
            } else if value.isEqualToString(NSPasteboardTypeTIFF) {
                OsString::from("output.tiff")
            } else if value.isEqualToString(NSPasteboardTypePNG) {
                OsString::from("output.png")
            } else if value.isEqualToString(NSPasteboardTypeRTF) {
                OsString::from("output.rtf")
            } else if value.isEqualToString(NSPasteboardTypeRTFD) {
                OsString::from("output.rtfd")
            } else if value.isEqualToString(NSPasteboardTypeHTML) {
                OsString::from("output.html")
            } else {
                OsString::from("")
            }
        }
    }

    fn get_file_type() -> Vec<&'static NSPasteboardType> /*Only return static constant*/ {
        unsafe {
            vec![
                NSPasteboardTypePDF,
                NSPasteboardTypeTIFF,
                NSPasteboardTypePNG,
                NSPasteboardTypeRTF,
                NSPasteboardTypeRTFD,
                NSPasteboardTypeHTML,
            ]
        }
    }

    fn get_text_type() -> Vec<&'static NSPasteboardType> /*Only return static constant*/ {
        unsafe {
            vec![
                NSPasteboardTypeString,
                NSPasteboardTypeMultipleTextSelection,
                NSPasteboardTypeURL,
            ]
        }
    }

    fn get_file_url_type() -> Vec<&'static NSPasteboardType> /*Only return static constant*/ {
        unsafe { vec![NSPasteboardTypeFileURL] }
    }

    fn get_unsupported_type() -> Vec<&'static NSPasteboardType> /*Only return static constant*/ {
        unsafe {
            vec![
                NSPasteboardTypeTabularText,
                NSPasteboardTypeFont,
                NSPasteboardTypeRuler,
                NSPasteboardTypeColor,
                NSPasteboardTypeTextFinderOptions,
                NSPasteboardTypeSound,
            ]
        }
    }

    #[allow(non_snake_case, unused)]
    fn get_NSPasteboardType(self) -> Vec<&'static NSPasteboardType> /*Only return static constant*/
    {
        match self {
            Self::File(..) => Self::get_file_type(),
            Self::Text(_) => Self::get_text_type(),
            Self::FileUrl(_) => Self::get_file_url_type(),
            _ => Self::get_unsupported_type(),
        }
    }

    pub fn new(item: Retained<NSPasteboardItem>) -> Self {
        unsafe {
            let all_type = item.types();
            let mut file = all_type.iter().filter(|x| {
                Self::get_file_type()
                    .iter()
                    .any(|curr| curr.isEqualToString(x))
            });
            let mut url = all_type.iter().filter(|x| {
                Self::get_file_url_type()
                    .iter()
                    .any(|curr| curr.isEqualToString(x))
            });
            let mut text = all_type.iter().filter(|x| {
                Self::get_text_type()
                    .iter()
                    .any(|curr| curr.isEqualToString(x))
            });

            if let Some(x) = file.next() {
                Self::File(item.dataForType(x).unwrap(), Self::get_extension(x))
            } else if let Some(x) = url.next() {
                let path = NSURL::URLWithDataRepresentation_relativeToURL(
                    &item.dataForType(&x).unwrap(),
                    None,
                );
                Self::FileUrl(path.relativePath().unwrap())
            } else if let Some(x) = text.next() {
                Self::Text(item.stringForType(&x).unwrap())
            } else {
                Self::Unsupported()
            }
        }
    }
}

impl From<Retained<NSPasteboardItem>> for Item {
    fn from(value: Retained<NSPasteboardItem>) -> Self {
        Self::new(value)
    }
}

pub fn get_count() -> isize {
    let _read_lock = PASTEBOARD_LOCK.read().expect("Lock poisoned");

    unsafe { NSPasteboard::generalPasteboard().changeCount() }
}

pub fn read() -> Option<Item> {
    let _read_lock = PASTEBOARD_LOCK.read().expect("Lock poisoned");

    debug!("Reading NSPasteboard...");
    unsafe {
        let board = NSPasteboard::generalPasteboard();
        debug!("Pasteboard: {}", board.name());
        debug!("Pasteboard change count: {}", board.changeCount());
        let items = board.pasteboardItems()?;
        debug!("Number of items: {}", items.count());
        if items.len() > 1 {
            warn!("More than 1 item in NSPasteBoard, selecting the last item by default")
        }
        if let Some(item) = items.last() {
            Some(Item::from(item.retain()))
        } else {
            info!("No items on the pasteboard.");
            None
        }
    }
}

pub fn write_file_url(file_url: Retained<NSURL>) {
    let _write_lock = PASTEBOARD_LOCK.write().expect("Lock poisoned");
    let board = unsafe { NSPasteboard::generalPasteboard() };

    let _ = unsafe { board.clearContents() };
    let obj = ProtocolObject::from_retained(file_url);
    let objects = NSArray::from_vec(vec![obj]);
    let res = unsafe { board.writeObjects(&objects) };
    if !res {
        panic!("Failed writing to pasteboard");
    }
}

pub fn write_text(text: String) {
    let _write_lock = PASTEBOARD_LOCK.write().expect("Lock poisoned");
    let board = unsafe { NSPasteboard::generalPasteboard() };

    let _ = unsafe { board.clearContents() };
    let s = NSString::from_str(&text);
    let obj = ProtocolObject::from_retained(s);
    let objects = NSArray::from_vec(vec![obj]);
    let res = unsafe { board.writeObjects(&objects) };
    if !res {
        panic!("Failed writing to pasteboard");
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::item::*;

    #[test]
    fn write_file_url_test() {
        let curr_dir = std::env::current_dir().unwrap();
        let file = "test.pdf";
        let path = curr_dir.join(format!("tests/files/{file}"));
        println!("{}", path.display());
        let s = NSString::from_str(path.as_path().to_str().unwrap());
        unsafe {
            let url = NSURL::fileURLWithPath(&s);
            write_file_url(url);
            let item = read().unwrap();
            let item = TransferableItem::try_from(item).unwrap();
            item.write_to_dir(std::env::current_dir().unwrap());
            std::fs::remove_file(std::env::current_dir().unwrap().join(PathBuf::from(file)))
                .unwrap();
        }
    }
}
