//! Observe directory flush support through safe Rust standard-library APIs.
#![forbid(unsafe_code)]

#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use std::{
        fs::{self, File, OpenOptions},
        io::Write,
        os::windows::fs::OpenOptionsExt,
    };
    const BACKUP_SEMANTICS: u32 = 0x02000000;
    let root = std::env::temp_dir().join(format!("vecnook-directory-probe-{}", std::process::id()));
    fs::create_dir(&root)?;
    let mut file = File::create(root.join("first.tmp"))?;
    file.write_all(b"synced file data")?;
    file.sync_all()?;
    fs::rename(root.join("first.tmp"), root.join("committed.bin"))?;
    file.sync_all()?;
    drop(file);
    for (name, read, write) in [
        ("read", true, false),
        ("write", false, true),
        ("read-write", true, true),
    ] {
        let result = OpenOptions::new()
            .read(read)
            .write(write)
            .custom_flags(BACKUP_SEMANTICS)
            .open(&root)
            .and_then(|directory| directory.sync_all());
        println!("directory access={name}, open+sync_all={result:?}");
        if name == "write" {
            result?;
        }
    }
    fs::remove_dir_all(root)?;
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    println!("Directory flush probe runs on Windows.");
}
