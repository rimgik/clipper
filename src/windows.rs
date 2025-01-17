use arboard::{Clipboard};

pub fn read_text() -> Result<String, Error>{
    let mut clipboard = Clipboard::new().unwrap();
    match clipboard.get_text() {
        Err(_) => Err(Error::Unsupported),
        Ok(text) => Ok(text)
    }
}

pub fn write_text(text: String) {
    let mut clipboard = Clipboard::new().unwrap();
    clipboard.set_text(text).unwrap();

}

#[derive(Hash, Debug)]
pub enum Error {
    Unsupported,
}