use super::*;
use std::io::Write;
fn png(width:u32,height:u32)->Vec<u8>{
    let mut bytes=vec![137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82];bytes.extend_from_slice(&width.to_be_bytes());bytes.extend_from_slice(&height.to_be_bytes());bytes.extend_from_slice(&[8,6,0,0,0,0,0,0,0]);bytes
}
fn jpeg(width:u16,height:u16)->Vec<u8>{
    let mut bytes=vec![255,216,255,192,0,17,8];bytes.extend_from_slice(&height.to_be_bytes());bytes.extend_from_slice(&width.to_be_bytes());bytes.extend_from_slice(&[3,1,17,0,2,17,0,3,17,0,255,217]);bytes
}
fn gif(width:u16,height:u16)->Vec<u8>{
    let mut bytes=b"GIF89a".to_vec();bytes.extend_from_slice(&width.to_le_bytes());bytes.extend_from_slice(&height.to_le_bytes());bytes.extend_from_slice(&[0,0,0]);bytes
}
fn webp(chunk:&[u8;4],payload:&[u8])->Vec<u8>{
    let mut bytes=b"RIFF".to_vec();let total=20+payload.len()+(payload.len()&1);bytes.extend_from_slice(&((total-8)as u32).to_le_bytes());bytes.extend_from_slice(b"WEBP");bytes.extend_from_slice(chunk);bytes.extend_from_slice(&(payload.len()as u32).to_le_bytes());bytes.extend_from_slice(payload);if payload.len()&1!=0{bytes.push(0);}bytes
}
fn webpx(width:u32,height:u32)->Vec<u8>{
    let mut payload=vec![0;4];payload.extend_from_slice(&(width-1).to_le_bytes()[..3]);payload.extend_from_slice(&(height-1).to_le_bytes()[..3]);webp(b"VP8X",&payload)
}
fn admitted(kind:MediaType,bytes:&[u8])->bool{
    let mut file=tempfile::tempfile().unwrap();file.write_all(bytes).unwrap();let accepted=allowed(&mut file,bytes.len()as u64,kind);assert_eq!(file.stream_position().unwrap(),0);accepted
}
#[test]
fn finite_image_headers_supply_bounded_dimensions_without_pixel_decoding(){
    for (kind,bytes) in [(MediaType::Png,png(1024,768)),(MediaType::Jpeg,jpeg(1024,768)),(MediaType::Gif,gif(1024,768)),(MediaType::Webp,webpx(1024,768))]{assert_eq!(dimensions(kind,&bytes,bytes.len()as u64),Some((1024,768)));assert!(admitted(kind,&bytes));}
    let bits=1023u32|(767u32<<14);let mut lossless=vec![47];lossless.extend_from_slice(&bits.to_le_bytes());assert!(admitted(MediaType::Webp,&webp(b"VP8L",&lossless)));
    let mut lossy=vec![0,0,0,157,1,42];lossy.extend_from_slice(&1024u16.to_le_bytes());lossy.extend_from_slice(&768u16.to_le_bytes());assert!(admitted(MediaType::Webp,&webp(b"VP8 ",&lossy)));
}
#[test]
fn side_pixel_and_encoded_caps_refuse_zero_oversized_or_truncated_images(){
    for (kind,bytes) in [(MediaType::Png,png(0,1)),(MediaType::Png,png(8193,1)),(MediaType::Jpeg,jpeg(8192,2049)),(MediaType::Gif,gif(8193,1)),(MediaType::Webp,webpx(8192,2049))]{assert!(!admitted(kind,&bytes));}
    assert!(admitted(MediaType::Png,&png(8192,2048)));
    let mut file=tempfile::tempfile().unwrap();file.write_all(&png(1,1)).unwrap();assert!(!allowed(&mut file,MAX_IMAGE_PREVIEW_BYTES+1,MediaType::Png));
    for bytes in [b"<svg>active</svg>".to_vec(),png(1,1)[..23].to_vec(),vec![255,216,255,192,0,1]]{assert!(!admitted(MediaType::Png,&bytes));}
    assert!(!admitted(MediaType::Jpeg,&[255,216,255,192,0,1]));
    let mut corrupted=webpx(1,1);corrupted[4]=0;assert!(!admitted(MediaType::Webp,&corrupted));
}
#[test]
fn jpeg_metadata_scanning_is_bounded_and_does_not_accept_other_sof_or_scan_markers(){
    let mut bytes=vec![255,216,255,225,0,5,1,2,3];bytes.extend_from_slice(&jpeg(32,64)[2..]);assert!(admitted(MediaType::Jpeg,&bytes));
    let mut bytes=vec![255,216];for _ in 0..4{bytes.extend_from_slice(&[255,225,255,255]);bytes.resize(bytes.len()+65533,0);}bytes.extend_from_slice(&jpeg(1,1)[2..]);assert!(!admitted(MediaType::Jpeg,&bytes));
    for marker in [195,218]{let mut bytes=jpeg(1,1);bytes[3]=marker;assert!(!admitted(MediaType::Jpeg,&bytes));}
}
