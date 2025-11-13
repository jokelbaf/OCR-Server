use actix_multipart::MultipartError;
use actix_multipart::form::{MultipartForm, MultipartFormConfig, tempfile::TempFile};
use actix_web::{
    App, HttpRequest, HttpResponse, HttpServer, Responder, Result,
    dev::ServiceResponse,
    get,
    http::header,
    middleware::{ErrorHandlerResponse, ErrorHandlers, Logger},
    post, web,
};
use log::{error, info};
use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use reqwest::get as fetch;
use rten::Model;
use serde::Serialize;
use serde_json;
use std::error::Error;


#[derive(Serialize)]
struct ApiResponse<T> {
    status: u16,
    message: String,
    data: Option<T>,
}

fn ok_response<T: Serialize>(data: T) -> HttpResponse {
    HttpResponse::Ok().json(ApiResponse {
        status: 200,
        message: "OK".to_string(),
        data: Some(data),
    })
}

fn error_response(status: u16, message: &str) -> HttpResponse {
    HttpResponse::build(
        actix_web::http::StatusCode::from_u16(status)
            .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR),
    )
    .json(ApiResponse::<()> {
        status,
        message: message.to_string(),
        data: None,
    })
}

fn handle_multipart_error(err: MultipartError, _req: &HttpRequest) -> actix_web::Error {
    error!("Multipart error: {}", err);
    let resp = HttpResponse::BadRequest().json(ApiResponse::<()> {
        status: 400,
        message: err.to_string(),
        data: None,
    });
    actix_web::error::InternalError::from_response(err, resp).into()
}

fn get_message_by_status(status: u16) -> String {
    match status {
        404 => "Not Found".to_string(),
        _ => "Something went wrong".to_string(),
    }
}

fn global_error_handler<B>(res: ServiceResponse<B>) -> Result<ErrorHandlerResponse<B>> {
    let (req, rsp) = res.into_parts();

    let status = rsp.status().as_u16();
    let message = match rsp.error() {
        Some(err) => format!("{}", err),
        None => get_message_by_status(status),
    };

    let response = serde_json::json!({
        "status": status,
        "message": message,
        "data": null,
    })
    .to_string();

    let new_response = HttpResponse::build(rsp.status())
        .insert_header((header::CONTENT_TYPE, "application/json"))
        .body(response);

    let res = ServiceResponse::new(req, new_response)
        .map_into_boxed_body()
        .map_into_right_body();

    Ok(ErrorHandlerResponse::Response(res))
}

struct AppState {
    engine: OcrEngine,
}

#[derive(Debug, MultipartForm)]
struct UploadForm {
    #[multipart(limit = "15MB")]
    file: TempFile,
}

#[post("/v1/recognize")]
async fn recognize(
    state: web::Data<AppState>,
    MultipartForm(form): MultipartForm<UploadForm>,
) -> impl Responder {
    let engine = &state.engine;

    let image_bytes = match std::fs::read(&form.file.file) {
        Ok(bytes) => bytes,
        Err(err) => {
            error!("Failed to read uploaded image: {}", err);
            return error_response(400, "Failed to read uploaded image");
        }
    };

    let img = match image::load_from_memory(&image_bytes) {
        Ok(image) => image.into_rgb8(),
        Err(err) => {
            error!("Invalid image format: {}", err);
            return error_response(400, "Invalid image format");
        }
    };

    let img_source = match ImageSource::from_bytes(img.as_raw(), img.dimensions()) {
        Ok(src) => src,
        Err(err) => {
            error!("Failed to process image: {}", err);
            return error_response(500, "Failed to process image");
        }
    };

    let ocr_input = match engine.prepare_input(img_source) {
        Ok(input) => input,
        Err(err) => {
            error!("Failed to prepare OCR input: {}", err);
            return error_response(500, "Failed to prepare OCR input");
        }
    };

    let word_rects = match engine.detect_words(&ocr_input) {
        Ok(rects) => rects,
        Err(err) => {
            error!("Failed to detect words: {}", err);
            return error_response(500, "Failed to detect words");
        }
    };

    let line_rects = engine.find_text_lines(&ocr_input, &word_rects);
    let line_texts = match engine.recognize_text(&ocr_input, &line_rects) {
        Ok(texts) => texts,
        Err(err) => {
            error!("Failed to recognize text: {}", err);
            return error_response(500, "Failed to recognize text");
        }
    };

    let recognized_text: Vec<String> = line_texts
        .iter()
        .flatten()
        .filter(|l| l.to_string().len() > 1)
        .map(|l| l.to_string())
        .collect();

    info!(
        "Successfully recognized text; found {} lines",
        recognized_text.len()
    );
    ok_response(recognized_text)
}

#[get("/health")]
async fn health() -> impl Responder {
    HttpResponse::Ok().body("OK")
}

#[actix_web::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let detection_model_url = "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten";
    let rec_model_url = "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten";

    let detection_model_bytes = download_model(detection_model_url).await?;
    let rec_model_bytes = download_model(rec_model_url).await?;

    let detection_model = Model::load(detection_model_bytes).map_err(|e| {
        error!("Failed to load detection model: {}", e);
        e
    })?;

    let recognition_model = Model::load(rec_model_bytes).map_err(|e| {
        error!("Failed to load recognition model: {}", e);
        e
    })?;

    let engine = OcrEngine::new(OcrEngineParams {
        detection_model: Some(detection_model),
        recognition_model: Some(recognition_model),
        ..Default::default()
    })
    .map_err(|e| {
        error!("Failed to initialize OCR engine: {}", e);
        e
    })?;

    let app_state = web::Data::new(AppState { engine });

    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .wrap(ErrorHandlers::new().default_handler(global_error_handler))
            .app_data(
                MultipartFormConfig::default()
                    .total_limit(15 * 1024 * 1024) // 15 MB
                    .memory_limit(15 * 1024 * 1024) // 15 MB
                    .error_handler(handle_multipart_error),
            )
            .app_data(app_state.clone())
            .service(recognize)
            .service(health)
    })
    .bind("0.0.0.0:6622")?
    .run()
    .await?;

    Ok(())
}

async fn download_model(url: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let response = fetch(url).await?;

    if response.status().is_success() {
        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    } else {
        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to download model from {}", url),
        )))
    }
}
