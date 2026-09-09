//! Detecção de rostos em frames, usada pelo enquadramento inteligente.
//!
//! Usa o YuNet, detector oficial do OpenCV Zoo (modelo ONNX de 230 KB embutido
//! no binário via `include_bytes!`, licença MIT), executado pelo `tract`, um
//! runtime ONNX em Rust puro: nada de OpenCV instalado no sistema. Depende da
//! feature opcional `vision`. Sem ela `load_face_detector` devolve um erro claro
//! dizendo como habilitar.

use std::path::Path;

use crate::core::errors::{ErrorCode, ToolError, ToolResult};

/// Modelo YuNet embutido no binário.
pub const MODEL_BYTES: &[u8] = include_bytes!("models/face_detection_yunet_2023mar.onnx");
#[cfg_attr(not(feature = "vision"), allow(dead_code))]
const SCORE_THRESHOLD: f32 = 0.6;
#[cfg_attr(not(feature = "vision"), allow(dead_code))]
const NMS_THRESHOLD: f32 = 0.3;
#[cfg_attr(not(feature = "vision"), allow(dead_code))]
const TOP_K: usize = 50;

/// Retângulo de um rosto detectado, em pixels do frame analisado.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub score: f64,
}

impl FaceBox {
    /// Centro horizontal do rosto.
    pub fn center_x(&self) -> f64 {
        self.x + self.width / 2.0
    }

    /// Centro vertical do rosto.
    pub fn center_y(&self) -> f64 {
        self.y + self.height / 2.0
    }

    /// Área do retângulo, usada para escolher o rosto principal.
    pub fn area(&self) -> f64 {
        self.width * self.height
    }
}

/// Detector YuNet: leve, roda em CPU e não precisa de download.
pub struct FaceDetector {
    #[cfg(feature = "vision")]
    model: yunet::Model,
}

impl std::fmt::Debug for FaceDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FaceDetector").finish_non_exhaustive()
    }
}

impl FaceDetector {
    /// Maior rosto no frame, ou `None` se não houver nenhum.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] se a imagem não puder ser lida.
    #[cfg(feature = "vision")]
    pub fn largest_face(&self, image_path: &Path) -> ToolResult<Option<FaceBox>> {
        let image = image::open(image_path).map_err(|error| {
            ToolError::new(
                format!(
                    "Não foi possível ler o frame {}: {error}",
                    image_path.display()
                ),
                ErrorCode::InvalidArgument,
            )
        })?;
        let boxes = yunet::detect(&self.model, &image.to_rgb8())?;
        Ok(boxes
            .into_iter()
            .max_by(|a, b| a.area().total_cmp(&b.area())))
    }

    /// Sem a feature `vision` o detector nem pode ser construído.
    #[cfg(not(feature = "vision"))]
    pub fn largest_face(&self, _image_path: &Path) -> ToolResult<Option<FaceBox>> {
        Err(unavailable())
    }
}

/// Carrega o modelo YuNet embutido.
///
/// # Errors
///
/// [`ErrorCode::Unavailable`] se o binário foi compilado sem a feature `vision`
/// ou o modelo não puder ser carregado.
pub fn load_face_detector() -> ToolResult<FaceDetector> {
    #[cfg(feature = "vision")]
    {
        let model = yunet::load().map_err(|error| {
            ToolError::with_hint(
                format!("Modelo de detecção de rosto não pôde ser carregado: {error}"),
                ErrorCode::Unavailable,
                "Reinstale o projeto; o modelo YuNet vem embutido em core/models.",
            )
        })?;
        Ok(FaceDetector { model })
    }
    #[cfg(not(feature = "vision"))]
    {
        Err(unavailable())
    }
}

#[cfg(not(feature = "vision"))]
fn unavailable() -> ToolError {
    ToolError::with_hint(
        "Detecção de rosto indisponível: binário compilado sem a feature 'vision'.",
        ErrorCode::Unavailable,
        "Use o binário completo (cargo build --release --features full) ou use mode='center' \
         em smart_crop.",
    )
}

#[cfg(feature = "vision")]
mod yunet {
    //! Pré e pós-processamento do YuNet, espelhando o `FaceDetectorYN` do OpenCV.

    use image::RgbImage;
    use tract_onnx::prelude::*;

    use super::{FaceBox, NMS_THRESHOLD, SCORE_THRESHOLD, TOP_K};
    use crate::core::errors::{ErrorCode, ToolError, ToolResult};

    /// O modelo embutido tem entrada fixa 640x640; frames menores são
    /// preenchidos com zeros e frames maiores reduzidos proporcionalmente.
    pub const INPUT_SIZE: usize = 640;
    const STRIDES: [usize; 3] = [8, 16, 32];

    pub type Model = std::sync::Arc<TypedRunnableModel>;

    pub fn load() -> TractResult<Model> {
        let mut cursor = std::io::Cursor::new(super::MODEL_BYTES);
        tract_onnx::onnx()
            .model_for_read(&mut cursor)?
            .with_input_fact(0, f32::fact([1, 3, INPUT_SIZE, INPUT_SIZE]).into())?
            .into_optimized()?
            .into_runnable()
    }

    /// Roda o detector em um frame RGB e devolve os rostos acima do limiar.
    pub fn detect(model: &Model, image: &RgbImage) -> ToolResult<Vec<FaceBox>> {
        let (width, height) = image.dimensions();
        if width < 2 || height < 2 {
            return Ok(Vec::new());
        }
        let scale = (INPUT_SIZE as f64 / f64::from(width))
            .min(INPUT_SIZE as f64 / f64::from(height))
            .min(1.0);
        let resized;
        let source = if scale < 1.0 {
            let new_w = ((f64::from(width) * scale).round() as u32).max(1);
            let new_h = ((f64::from(height) * scale).round() as u32).max(1);
            resized =
                image::imageops::resize(image, new_w, new_h, image::imageops::FilterType::Triangle);
            &resized
        } else {
            image
        };
        let input = to_tensor(source);
        let outputs = model.run(tvec!(input.into())).map_err(|error| {
            ToolError::new(format!("YuNet falhou: {error}"), ErrorCode::Unavailable)
        })?;
        let mut boxes = decode(&outputs)?;
        boxes = nms(boxes);
        for face in &mut boxes {
            face.x /= scale;
            face.y /= scale;
            face.width /= scale;
            face.height /= scale;
        }
        Ok(boxes)
    }

    /// Imagem BGR em `[1, 3, 640, 640]`, sem normalização, como o `blobFromImage`.
    fn to_tensor(image: &RgbImage) -> Tensor {
        let (width, height) = image.dimensions();
        let mut array = tract_ndarray::Array4::<f32>::zeros((1, 3, INPUT_SIZE, INPUT_SIZE));
        for (x, y, pixel) in image.enumerate_pixels() {
            let (x, y) = (x as usize, y as usize);
            if x >= INPUT_SIZE || y >= INPUT_SIZE {
                continue;
            }
            array[[0, 0, y, x]] = f32::from(pixel[2]);
            array[[0, 1, y, x]] = f32::from(pixel[1]);
            array[[0, 2, y, x]] = f32::from(pixel[0]);
        }
        debug_assert!(width as usize <= INPUT_SIZE && height as usize <= INPUT_SIZE);
        array.into()
    }

    fn decode(outputs: &TVec<TValue>) -> ToolResult<Vec<FaceBox>> {
        if outputs.len() < 12 {
            return Err(ToolError::new(
                "YuNet devolveu menos saídas que o esperado.",
                ErrorCode::Unavailable,
            ));
        }
        let view = |index: usize| -> ToolResult<Vec<f32>> {
            let tensor: &Tensor = &outputs[index];
            tensor
                .try_as_plain()
                .and_then(|view| view.as_slice::<f32>().map(<[f32]>::to_vec))
                .map_err(|error| ToolError::new(format!("YuNet: {error}"), ErrorCode::Unavailable))
        };
        let mut faces = Vec::new();
        for (i, stride) in STRIDES.iter().enumerate() {
            let cols = INPUT_SIZE / stride;
            let rows = INPUT_SIZE / stride;
            let cls = view(i)?;
            let obj = view(i + 3)?;
            let bbox = view(i + 6)?;
            for r in 0..rows {
                for c in 0..cols {
                    let idx = r * cols + c;
                    let (Some(cls_score), Some(obj_score)) = (cls.get(idx), obj.get(idx)) else {
                        continue;
                    };
                    let score = (cls_score.clamp(0.0, 1.0) * obj_score.clamp(0.0, 1.0)).sqrt();
                    if score < SCORE_THRESHOLD {
                        continue;
                    }
                    let base = idx * 4;
                    if base + 3 >= bbox.len() {
                        continue;
                    }
                    let stride = *stride as f32;
                    let cx = (c as f32 + bbox[base]) * stride;
                    let cy = (r as f32 + bbox[base + 1]) * stride;
                    let w = bbox[base + 2].exp() * stride;
                    let h = bbox[base + 3].exp() * stride;
                    faces.push(FaceBox {
                        x: f64::from(cx - w / 2.0),
                        y: f64::from(cy - h / 2.0),
                        width: f64::from(w),
                        height: f64::from(h),
                        score: f64::from(score),
                    });
                }
            }
        }
        Ok(faces)
    }

    /// Supressão de não-máximos, como o `NMSBoxes` do OpenCV com `top_k`.
    fn nms(mut boxes: Vec<FaceBox>) -> Vec<FaceBox> {
        boxes.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut kept: Vec<FaceBox> = Vec::new();
        for candidate in boxes {
            if kept.len() >= TOP_K {
                break;
            }
            let overlaps = kept
                .iter()
                .any(|k| iou(k, &candidate) > f64::from(NMS_THRESHOLD));
            if !overlaps {
                kept.push(candidate);
            }
        }
        kept
    }

    fn iou(a: &FaceBox, b: &FaceBox) -> f64 {
        let x1 = a.x.max(b.x);
        let y1 = a.y.max(b.y);
        let x2 = (a.x + a.width).min(b.x + b.width);
        let y2 = (a.y + a.height).min(b.y + b.height);
        let inter = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
        let union = a.area() + b.area() - inter;
        if union <= 0.0 {
            0.0
        } else {
            inter / union
        }
    }
}
