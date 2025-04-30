use pt::base::{try_into_tarfile, ImageInfo, TarImage};

#[test]
fn test_pt_tar() {
    //let path = r#"X:\gta5\tools_ng\bin\python\App\Lib\test\testtar.tar"#;
    //let path = r#"C:\Program Files\Docker\Docker\resources\wsl\wsl-data.tar"#;
    let path = r#"C:\Program Files\Docker\Docker\resources\wsl\wsl-bootstrap.tar"#;
    let img = match TarImage::open(path) {
        Ok(img) => img,
        Err(e) => {
            println!("Error opening tar file: {}", e);
            return;
        }
    };
    match img.try_lock().unwrap().for_each_entry(|file| {
        let tarfile = try_into_tarfile(file).unwrap();
        println!("{} : {} {} {}", tarfile.get_offset(),tarfile.get_name(), tarfile.get_size(), tarfile.get_type_flag());
        Ok(())
    }) {
        Ok(_) => println!("Successfully iterated through tar entries."),
        Err(e) => println!("Error iterating through tar entries: {}", e),
    };
}