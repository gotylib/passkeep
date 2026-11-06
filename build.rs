use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rgba");

    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rgba = fs::read(manifest.join("src/icon.rgba")).expect("src/icon.rgba");
    let ico = make_ico(&rgba);
    let ico_path = manifest.join("target").join("passkeep.ico");
    fs::create_dir_all(ico_path.parent().unwrap()).unwrap();
    fs::write(&ico_path, &ico).unwrap();

    if env::var("CARGO_CFG_TARGET_OS").ok().as_deref() == Some("windows") {
        embed_windows_icon(&ico_path);
    }
}

fn embed_windows_icon(path: &Path) {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon(path.to_str().unwrap());
        res.set("ProductName", "passkeep");
        res.set("FileDescription", "passkeep");
        res.set("ProductVersion", "0.1.0.0");
        res.set("FileVersion", "0.1.0.0");
        res.set("OriginalFilename", "passkeep.exe");
        res.compile().unwrap();
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
}

fn make_ico(rgba: &[u8]) -> Vec<u8> {
    const SOURCE: u32 = 256;
    assert_eq!(rgba.len(), (SOURCE * SOURCE * 4) as usize);
    let sizes = [16u32, 24, 32, 48, 64, 256];
    let images: Vec<Vec<u8>> = sizes.iter().map(|&size| dib(rgba, size)).collect();

    let mut file = Vec::new();
    file.extend_from_slice(&0u16.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&(images.len() as u16).to_le_bytes());

    let mut offset = 6 + images.len() * 16;
    for (size, image) in sizes.iter().zip(&images) {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        file.push(dim);
        file.push(dim);
        file.push(0);
        file.push(0);
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&32u16.to_le_bytes());
        file.extend_from_slice(&(image.len() as u32).to_le_bytes());
        file.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += image.len();
    }
    for image in &images {
        file.extend_from_slice(image);
    }
    file
}

fn dib(rgba: &[u8], size: u32) -> Vec<u8> {
    let pixels = resample(rgba, size);
    let stride = ((size + 31) / 32) * 4;
    let xor_bytes = (size * size * 4) as usize;
    let and_bytes = (stride * size) as usize;
    let mut blob = Vec::with_capacity(40 + xor_bytes + and_bytes);

    blob.extend_from_slice(&40u32.to_le_bytes());
    blob.extend_from_slice(&(size as i32).to_le_bytes());
    blob.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    blob.extend_from_slice(&1u16.to_le_bytes());
    blob.extend_from_slice(&32u16.to_le_bytes());
    blob.extend_from_slice(&0u32.to_le_bytes());
    blob.extend_from_slice(&(xor_bytes as u32).to_le_bytes());
    blob.extend_from_slice(&0u32.to_le_bytes());
    blob.extend_from_slice(&0u32.to_le_bytes());
    blob.extend_from_slice(&0u32.to_le_bytes());
    blob.extend_from_slice(&0u32.to_le_bytes());

    let mut mask = vec![0u8; and_bytes];
    for y in (0..size).rev() {
        for x in 0..size {
            let pixel = pixels[(y * size + x) as usize];
            blob.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
            if pixel[3] < 128 {
                let row = (size - 1 - y) * stride + x / 8;
                mask[row as usize] |= 0x80 >> (x % 8);
            }
        }
    }
    blob.extend_from_slice(&mask);
    blob
}

fn resample(rgba: &[u8], size: u32) -> Vec<[u8; 4]> {
    const SOURCE: u32 = 256;
    let mut pixels = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        let y0 = y * SOURCE / size;
        let y1 = ((y + 1) * SOURCE / size).max(y0 + 1).min(SOURCE);
        for x in 0..size {
            let x0 = x * SOURCE / size;
            let x1 = ((x + 1) * SOURCE / size).max(x0 + 1).min(SOURCE);
            let mut sum = [0u32; 4];
            let mut count = 0u32;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let index = ((sy * SOURCE + sx) * 4) as usize;
                    for channel in 0..4 {
                        sum[channel] += rgba[index + channel] as u32;
                    }
                    count += 1;
                }
            }
            pixels.push(std::array::from_fn(|channel| (sum[channel] / count) as u8));
        }
    }
    pixels
}
