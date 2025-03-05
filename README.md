# OCR-Server

Optical character recognition server written in Rust.

## Development

Always use release mode; debug target won't work well with `ocrs` library.

**Building:**
```bash
cargo build --release
```
**Running:**
```bash
cargo run --release
```

## Usage

Recognize text from `image.png` file:
```bash
curl -X POST -F "file=@image.png" http://localhost:6444/v1/recognize
```
