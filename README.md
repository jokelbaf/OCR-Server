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

Here are a few examples of how to use the server.

### CLI
```bash
curl -X POST -F "file=@image.png" https://ocr.pkg.rs/v1/recognize
```

### Python
```python
import asyncio
import io
import typing

import aiohttp
from PIL import Image

async def run_remote_ocr(image: str | bytes | Image.Image) -> list[str]:
    """Connect to the remote OCR service and extract text from the image."""
    if isinstance(image, str):
        with open(image, "rb") as f:
            image_bytes = f.read()
    elif isinstance(image, Image.Image):
        with io.BytesIO() as output:
            image.save(output, format="PNG")
            image_bytes = output.getvalue()
    elif isinstance(image, bytes):
        image_bytes = image
    else:
        raise ValueError("Unsupported image type")

    url = "https://ocr.pkg.rs/v1/recognize"

    async with aiohttp.ClientSession() as session:
        form_data = aiohttp.FormData()
        form_data.add_field(
            "file",
            image_bytes,
            filename="image.png",
            content_type="image/png"
        )

        async with session.post(url, data=form_data) as response:
            data = await response.json()

            if response.status == 200 and data.get("status") == 200:
                return data.get("data", [])
            else:
                raise RuntimeError(data.get(
                    "message", "Remote OCR failed to process the image"
                ))

async def main():
    image_path = "image.png"
    result = await run_remote_ocr(image_path)
    print(result)

if __name__ == "__main__":
    asyncio.run(main())
```
