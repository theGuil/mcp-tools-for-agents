"""Detecção de rostos em frames, usada pelo enquadramento inteligente.

Usa o YuNet, detector oficial do OpenCV Zoo (modelo ONNX de 230 KB embarcado
em ``core/models``, licença MIT). Depende do extra opcional ``vision``
(opencv-python-headless). Sem ele ``load_face_detector`` devolve um erro claro
dizendo como instalar.
"""

from __future__ import annotations

import importlib
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Final, Protocol, cast

from core.errors import ToolError

if TYPE_CHECKING:
    from collections.abc import Sequence

MODEL_PATH: Final = Path(__file__).parent / "models" / "face_detection_yunet_2023mar.onnx"
_SCORE_THRESHOLD: Final = 0.6
_NMS_THRESHOLD: Final = 0.3
_TOP_K: Final = 50
_MIN_DIM: Final = 2


class _Image(Protocol):
    shape: Sequence[int]


class _YuNet(Protocol):
    def setInputSize(self, size: tuple[int, int]) -> None: ...  # noqa: N802
    def detect(self, image: _Image) -> tuple[int, Sequence[Sequence[float]] | None]: ...


class _YuNetFactory(Protocol):
    def create(  # noqa: PLR0917  # espelha a assinatura posicional do OpenCV
        self,
        model: str,
        config: str,
        input_size: tuple[int, int],
        score_threshold: float,
        nms_threshold: float,
        top_k: int,
    ) -> _YuNet: ...


class _Cv2(Protocol):
    FaceDetectorYN: _YuNetFactory

    def imread(self, path: str) -> _Image | None: ...


@dataclass(frozen=True, slots=True)
class FaceBox:
    """Retângulo de um rosto detectado, em pixels do frame analisado."""

    x: float
    y: float
    width: float
    height: float
    score: float

    @property
    def center_x(self) -> float:
        """Centro horizontal do rosto."""
        return self.x + self.width / 2

    @property
    def center_y(self) -> float:
        """Centro vertical do rosto."""
        return self.y + self.height / 2

    @property
    def area(self) -> float:
        """Área do retângulo, usada para escolher o rosto principal."""
        return self.width * self.height


@dataclass(frozen=True, slots=True)
class FaceDetector:
    """Detector YuNet do OpenCV: leve, roda em CPU e não precisa de download."""

    cv2: _Cv2
    model: _YuNet

    def largest_face(self, image_path: Path) -> FaceBox | None:
        """Maior rosto no frame, ou ``None`` se não houver nenhum."""
        image = self.cv2.imread(str(image_path))
        if image is None or len(image.shape) < _MIN_DIM:
            return None
        height, width = int(image.shape[0]), int(image.shape[1])
        self.model.setInputSize((width, height))
        _, faces = self.model.detect(image)
        if faces is None:
            return None
        boxes = [
            FaceBox(float(f[0]), float(f[1]), float(f[2]), float(f[3]), float(f[-1])) for f in faces
        ]
        if not boxes:
            return None
        return max(boxes, key=lambda b: b.area)


def load_face_detector() -> FaceDetector:
    """Carrega o OpenCV e o modelo YuNet embarcado.

    Raises:
        ToolError: Se o OpenCV não estiver instalado ou o modelo não existir.
    """
    try:
        cv2 = cast("_Cv2", importlib.import_module("cv2"))
    except ModuleNotFoundError as exc:
        raise ToolError(
            "Detecção de rosto indisponível: OpenCV não instalado.",
            code="unavailable",
            hint="Instale com: uv sync --extra vision. Ou use mode='center' em smart_crop.",
        ) from exc
    if not MODEL_PATH.is_file():
        raise ToolError(
            f"Modelo de detecção de rosto não encontrado em {MODEL_PATH.name}.",
            code="unavailable",
            hint="Reinstale o projeto; o modelo YuNet vem em core/models.",
        )
    model = cv2.FaceDetectorYN.create(
        str(MODEL_PATH), "", (320, 320), _SCORE_THRESHOLD, _NMS_THRESHOLD, _TOP_K
    )
    return FaceDetector(cv2=cv2, model=model)
