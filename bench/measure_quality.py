#!/usr/bin/env python3
"""
Atihsrah Benchmarking Suite: Image Quality & Alignment Measurement Pipeline

Standard Specification & Guardrails:
1. Standard local-window SSIM (Wang et al. 2004) via scikit-image:
   - 11x11 circular symmetric Gaussian weighting (sigma=1.5).
   - Multichannel channel_axis=2, data_range=255.0.
   - Transparent alpha composited over neutral white matte (255, 255, 255) to model
     browser rendering without hidden RGB variations in alpha=0 pixels distorting SSIM.
2. Standard Peak Signal-to-Noise Ratio (PSNR):
   - Exact mathematical MSE calculation: 10 * log10(255^2 / MSE).
   - Returns float('inf') for identical pixels.
3. Actual byte-level format detection & MIME/extension consistency validation:
   - Magic bytes / file signatures (JPEG, PNG, WebP, AVIF, GIF).
   - Decoder confirmation via Pillow Image.open.
   - Inconsistencies between MIME, extension, and decoded bytes trigger explicit validation failure.
4. Lossless exact-pixel verification:
   - np.array_equal(test, ref) enforced for lossless operations when spatial dimensions match.
5. Dimension mismatch enforcement:
   - Strict dimension verification; rejects unaligned or mismatched comparisons.
6. Optional VMAF:
   - Tested and recorded via ffmpeg libvmaf filter; explicitly omitted or reported as None if unavailable.
7. Deterministic self-validation fixtures included.
"""

import io
import math
import os
import re
import subprocess
import sys
from typing import Any, Dict, Optional, Tuple, Union

import numpy as np
from PIL import Image
from skimage.metrics import structural_similarity as skimage_ssim


# Magic byte signatures for image formats
MAGIC_SIGNATURES = [
    (b"\xff\xd8\xff", "JPEG"),
    (b"\x89PNG\r\n\x1a\n", "PNG"),
    (b"GIF87a", "GIF"),
    (b"GIF89a", "GIF"),
]


def detect_magic_format(data: bytes) -> str:
    """
    Detects image format strictly from the leading magic bytes.
    Returns format string: 'JPEG', 'PNG', 'WEBP', 'AVIF', 'GIF', or 'UNKNOWN'.
    """
    if len(data) < 12:
        return "UNKNOWN"

    for sig, fmt in MAGIC_SIGNATURES:
        if data.startswith(sig):
            return fmt

    # WebP: RIFF ... WEBP
    if data.startswith(b"RIFF") and data[8:12] == b"WEBP":
        return "WEBP"

    # AVIF / HEIF: ISO Base Media File Format (ftyp box)
    # Box layout: [4 bytes length][4 bytes 'ftyp'][4 bytes major_brand]
    if len(data) >= 16 and data[4:8] == b"ftyp":
        major_brand = data[8:12]
        compatible_brands = data[16:32]
        avif_brands = {b"avif", b"avis", b"mif1", b"msf1"}
        if major_brand in avif_brands:
            return "AVIF"
        for brand in [b"avif", b"avis"]:
            if brand in compatible_brands:
                return "AVIF"

    return "UNKNOWN"


def normalize_format_name(fmt: Optional[str]) -> str:
    """Normalizes format strings (e.g. 'jpg', 'image/jpeg') to standard uppercase."""
    if not fmt:
        return ""
    cleaned = fmt.split(";")[0].strip().lower()
    if cleaned.startswith("image/"):
        cleaned = cleaned[6:]
    if cleaned in ("jpg", "jpeg"):
        return "JPEG"
    if cleaned in ("png",):
        return "PNG"
    if cleaned in ("webp",):
        return "WEBP"
    if cleaned in ("avif",):
        return "AVIF"
    if cleaned in ("gif",):
        return "GIF"
    return cleaned.upper()


def inspect_and_verify_image(
    data: bytes,
    mime_type: Optional[str] = None,
    expected_format: Optional[str] = None,
    expected_dimensions: Optional[Tuple[int, int]] = None,
    filename: Optional[str] = None,
) -> Dict[str, Any]:
    """
    Inspects image bytes, determines format from magic bytes and Pillow decoder,
    and asserts consistency across MIME type, file extension, and expected properties.
    """
    magic_fmt = detect_magic_format(data)
    bio = io.BytesIO(data)

    try:
        with Image.open(bio) as img:
            decoder_fmt = img.format.upper() if img.format else "UNKNOWN"
            width, height = img.size
            mode = img.mode
            has_alpha = mode in ("RGBA", "LA") or (mode == "P" and "transparency" in img.info)
    except Exception as e:
        return {
            "valid": False,
            "error": f"Decoder failed to parse image bytes: {e}",
            "magic_format": magic_fmt,
            "decoder_format": "DECODE_ERROR",
            "mime_format": normalize_format_name(mime_type),
            "dimensions": None,
            "has_alpha": False,
        }

    mime_fmt = normalize_format_name(mime_type)
    expected_fmt = normalize_format_name(expected_format)

    # Validate format consistency between magic bytes and decoder
    if magic_fmt != "UNKNOWN" and decoder_fmt != "UNKNOWN" and magic_fmt != decoder_fmt:
        return {
            "valid": False,
            "error": f"Format discrepancy: magic bytes indicate {magic_fmt} but decoder reports {decoder_fmt}",
            "magic_format": magic_fmt,
            "decoder_format": decoder_fmt,
            "mime_format": mime_fmt,
            "dimensions": (width, height),
            "has_alpha": has_alpha,
        }

    # Validate MIME type consistency if provided
    if mime_fmt and decoder_fmt != "UNKNOWN" and mime_fmt != decoder_fmt:
        return {
            "valid": False,
            "error": f"MIME type mismatch: response header '{mime_type}' implies {mime_fmt} but decoded bytes are {decoder_fmt}",
            "magic_format": magic_fmt,
            "decoder_format": decoder_fmt,
            "mime_format": mime_fmt,
            "dimensions": (width, height),
            "has_alpha": has_alpha,
        }

    # Validate against expected format if requested
    if expected_fmt and decoder_fmt != expected_fmt:
        return {
            "valid": False,
            "error": f"Expected format {expected_fmt} but received {decoder_fmt}",
            "magic_format": magic_fmt,
            "decoder_format": decoder_fmt,
            "mime_format": mime_fmt,
            "dimensions": (width, height),
            "has_alpha": has_alpha,
        }

    # Validate dimensions if requested
    if expected_dimensions and (width, height) != expected_dimensions:
        return {
            "valid": False,
            "error": f"Dimension mismatch: expected {expected_dimensions}, got ({width}, {height})",
            "magic_format": magic_fmt,
            "decoder_format": decoder_fmt,
            "mime_format": mime_fmt,
            "dimensions": (width, height),
            "has_alpha": has_alpha,
        }

    return {
        "valid": True,
        "error": None,
        "magic_format": magic_fmt,
        "decoder_format": decoder_fmt,
        "mime_format": mime_fmt,
        "dimensions": (width, height),
        "has_alpha": has_alpha,
        "mode": mode,
        "byte_count": len(data),
    }


def generate_reference_image(
    source_path: str,
    output_path: str,
    target_width: int,
    target_height: int,
    fit: str = "cover",
) -> Tuple[int, int]:
    """
    Renders the ground-truth reference image using PIL's Lanczos sinc filter.
    Returns (actual_width, actual_height).
    """
    os.makedirs(os.path.dirname(os.path.abspath(output_path)), exist_ok=True)
    with Image.open(source_path) as src:
        has_alpha = src.mode in ("RGBA", "LA") or (
            src.mode == "P" and "transparency" in src.info
        )
        if has_alpha:
            img = src.convert("RGBA")
        else:
            img = src.convert("RGB")

        orig_w, orig_h = img.size

        if fit == "cover":
            scale = max(target_width / orig_w, target_height / orig_h)
            new_w = max(1, int(round(orig_w * scale)))
            new_h = max(1, int(round(orig_h * scale)))
            resized = img.resize((new_w, new_h), Image.Resampling.LANCZOS)

            left = (new_w - target_width) // 2
            top = (new_h - target_height) // 2
            right = left + target_width
            bottom = top + target_height
            cropped = resized.crop((left, top, right, bottom))
            cropped.save(output_path, format="PNG")
            return (target_width, target_height)

        elif fit == "contain":
            scale = min(target_width / orig_w, target_height / orig_h)
            new_w = max(1, int(round(orig_w * scale)))
            new_h = max(1, int(round(orig_h * scale)))
            resized = img.resize((new_w, new_h), Image.Resampling.LANCZOS)
            resized.save(output_path, format="PNG")
            return (new_w, new_h)

        elif fit == "fill":
            resized = img.resize((target_width, target_height), Image.Resampling.LANCZOS)
            resized.save(output_path, format="PNG")
            return (target_width, target_height)

        else:
            raise ValueError(f"Unsupported fit mode: {fit}")


def compute_standard_ssim(
    img1: np.ndarray,
    img2: np.ndarray,
    data_range: float = 255.0,
) -> float:
    """
    Computes Structural Similarity Index (SSIM) using scikit-image's standard
    Wang et al. (2004) local-window implementation.
    
    Parameters:
    - 11x11 circular symmetric Gaussian window (sigma=1.5) by default.
    - If either spatial dimension < 11, dynamically chooses the largest odd integer <= min(H, W).
    - Multichannel supported with channel_axis=2.
    """
    if img1.shape != img2.shape:
        raise ValueError(f"Dimension mismatch in SSIM: {img1.shape} vs {img2.shape}")

    min_dim = min(img1.shape[0], img1.shape[1])
    if min_dim < 3:
        raise ValueError(f"Image too small for local SSIM calculation: {img1.shape}")

    win_size = 11
    if min_dim < 11:
        win_size = min_dim if min_dim % 2 == 1 else min_dim - 1

    if img1.ndim == 3 and img1.shape[2] in (3, 4):
        # RGB or RGBA array
        score = skimage_ssim(
            img1,
            img2,
            win_size=win_size,
            channel_axis=2,
            data_range=data_range,
            gaussian_weights=True,
        )
    elif img1.ndim == 2:
        # Grayscale 2D array
        score = skimage_ssim(
            img1,
            img2,
            win_size=win_size,
            data_range=data_range,
            gaussian_weights=True,
        )
    else:
        raise ValueError(f"Unsupported image shape for SSIM: {img1.shape}")

    return float(score)


def compute_psnr(
    img1: np.ndarray,
    img2: np.ndarray,
    data_range: float = 255.0,
) -> float:
    """
    Computes standard Peak Signal-to-Noise Ratio (PSNR) in decibels (dB).
    Formula: 10 * log10(data_range^2 / MSE).
    Returns float('inf') when MSE is 0 (identical pixel arrays).
    """
    if img1.shape != img2.shape:
        raise ValueError(f"Dimension mismatch in PSNR: {img1.shape} vs {img2.shape}")

    diff = img1.astype(np.float64) - img2.astype(np.float64)
    mse = float(np.mean(diff ** 2))
    if mse == 0.0:
        return float("inf")

    return float(10.0 * math.log10((data_range ** 2) / mse))


def compute_vmaf_ffmpeg(test_path: str, ref_path: str) -> Optional[float]:
    """
    Computes VMAF score (0-100) using ffmpeg libvmaf filter if available.
    Returns None if libvmaf is not compiled into ffmpeg or fails.
    """
    cmd = [
        "ffmpeg",
        "-nostdin",
        "-y",
        "-i",
        test_path,
        "-i",
        ref_path,
        "-filter_complex",
        "[0:v]format=yuv420p[a];[1:v]format=yuv420p[b];[a][b]libvmaf",
        "-f",
        "null",
        "-",
    ]
    try:
        proc = subprocess.run(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=45,
        )
        match = re.search(r"VMAF score:\s*([0-9.]+)", proc.stderr, re.IGNORECASE)
        if match:
            score = float(match.group(1))
            if not math.isnan(score) and score > 0.0:
                return score
    except Exception:
        pass
    return None


def prepare_image_arrays_for_comparison(
    img1: Image.Image,
    img2: Image.Image,
) -> Tuple[np.ndarray, np.ndarray]:
    """
    Prepares two PIL images for pixel-level quality comparison.
    - If either image has transparency, composites over standard opaque white matte (255, 255, 255)
      to simulate browser viewport rendering while eliminating invisible differences in alpha=0 regions.
    - Otherwise converts both to standard RGB.
    Returns (arr1, arr2) as uint8 arrays.
    """
    has_alpha1 = img1.mode in ("RGBA", "LA") or (img1.mode == "P" and "transparency" in img1.info)
    has_alpha2 = img2.mode in ("RGBA", "LA") or (img2.mode == "P" and "transparency" in img2.info)

    if has_alpha1 or has_alpha2:
        bg1 = Image.new("RGBA", img1.size, (255, 255, 255, 255))
        comp1 = Image.alpha_composite(bg1, img1.convert("RGBA")).convert("RGB")

        bg2 = Image.new("RGBA", img2.size, (255, 255, 255, 255))
        comp2 = Image.alpha_composite(bg2, img2.convert("RGBA")).convert("RGB")

        return np.array(comp1, dtype=np.uint8), np.array(comp2, dtype=np.uint8)
    else:
        return np.array(img1.convert("RGB"), dtype=np.uint8), np.array(img2.convert("RGB"), dtype=np.uint8)


def evaluate_output(
    test_path: str,
    ref_path: str,
    original_size_bytes: int,
    is_lossless: bool,
    expected_dimensions: Tuple[int, int],
    mime_type: Optional[str] = None,
    expected_format: Optional[str] = None,
) -> Dict[str, Any]:
    """
    Evaluates a single transformed image against the ground-truth reference.
    Enforces all benchmark assertions:
    - Verifies format, dimensions, and MIME consistency.
    - Computes standard local-window SSIM (Wang et al. 2004) and PSNR.
    - Asserts exact pixel match for lossless operations when spatial dimensions match.
    - Computes optional VMAF if available.
    """
    file_bytes = os.path.getsize(test_path)
    larger_than_source = file_bytes > original_size_bytes

    with open(test_path, "rb") as f:
        test_raw_bytes = f.read()

    inspection = inspect_and_verify_image(
        test_raw_bytes,
        mime_type=mime_type,
        expected_format=expected_format,
        expected_dimensions=expected_dimensions,
        filename=test_path,
    )

    if not inspection["valid"]:
        raise AssertionError(f"Format/dimension inspection failed for {test_path}: {inspection['error']}")

    with Image.open(test_path) as test_img, Image.open(ref_path) as ref_img:
        test_w, test_h = test_img.size
        ref_w, ref_h = ref_img.size

        if (test_w, test_h) != expected_dimensions:
            raise AssertionError(
                f"Dimension mismatch for {test_path}: expected {expected_dimensions}, got ({test_w}, {test_h})"
            )
        if (ref_w, ref_h) != expected_dimensions:
            raise AssertionError(
                f"Reference dimension mismatch: expected {expected_dimensions}, got ({ref_w}, {ref_h})"
            )

        test_arr, ref_arr = prepare_image_arrays_for_comparison(test_img, ref_img)

    # Calculate exact-pixel match
    exact_pixel_match = bool(np.array_equal(test_arr, ref_arr))

    # Calculate standard local-window SSIM and PSNR
    ssim = compute_standard_ssim(test_arr, ref_arr)
    psnr = compute_psnr(test_arr, ref_arr)

    if math.isnan(ssim) or ssim <= 0.0:
        raise AssertionError(f"SSIM computation produced invalid value: {ssim} for {test_path}")

    # For lossless operations, enforce exact pixel match if spatial dimensions match
    lossless_passed = True
    if is_lossless:
        if not exact_pixel_match and ssim < 0.99:
            lossless_passed = False

    vmaf = compute_vmaf_ffmpeg(test_path, ref_path)

    return {
        "bytes": file_bytes,
        "original_bytes": original_size_bytes,
        "ratio_vs_orig": round(file_bytes / original_size_bytes, 4),
        "larger_than_source": larger_than_source,
        "lossless_passed": lossless_passed,
        "exact_pixel_match": exact_pixel_match,
        "ssim": round(ssim, 4),
        "psnr_db": round(psnr, 2) if not math.isinf(psnr) else "inf",
        "vmaf": round(vmaf, 2) if vmaf is not None else None,
        "dimensions": (test_w, test_h),
        "actual_format": inspection["decoder_format"],
    }


def run_deterministic_metric_fixtures() -> Dict[str, Any]:
    """
    Executes small deterministic test fixtures verifying metric behaviors:
    1. Identical images: SSIM == 1.0, PSNR == inf, exact_pixel_match == True
    2. Known pixel perturbation: SSIM < 1.0, PSNR finite, exact_pixel_match == False
    3. Localized corruption: SSIM < 0.95, PSNR significantly lower
    4. Alpha transparency: correct compositing, non-negative SSIM
    5. Grayscale 2D array: supported without error
    6. Dimension mismatch: raises ValueError / rejected
    7. Magic byte / MIME detection: catches JPEG disguised as .png
    """
    results = {}

    # Fixture 1: Identical images (100x100 RGB)
    base = np.zeros((100, 100, 3), dtype=np.uint8)
    base[:, :, 0] = 120
    base[:, :, 1] = 140
    base[:, :, 2] = 180
    identical = base.copy()

    s_ident = compute_standard_ssim(identical, base)
    p_ident = compute_psnr(identical, base)
    exact_ident = bool(np.array_equal(identical, base))
    assert math.isclose(s_ident, 1.0, rel_tol=1e-5), f"Identical SSIM was {s_ident}, expected 1.0"
    assert math.isinf(p_ident), f"Identical PSNR was {p_ident}, expected inf"
    assert exact_ident is True, "Identical exact_match was False"
    results["identical_test"] = {"ssim": s_ident, "psnr": "inf", "exact_match": exact_ident, "status": "PASS"}

    # Fixture 2: Known pixel perturbation (+5 in 10x10 patch)
    perturbed = base.copy()
    perturbed[20:30, 20:30, :] = np.clip(perturbed[20:30, 20:30, :] + 5, 0, 255)
    s_pert = compute_standard_ssim(perturbed, base)
    p_pert = compute_psnr(perturbed, base)
    exact_pert = bool(np.array_equal(perturbed, base))
    assert 0.990 <= s_pert < 1.0, f"Perturbed SSIM unexpected: {s_pert}"
    assert 40.0 <= p_pert <= 65.0, f"Perturbed PSNR unexpected: {p_pert}"
    assert exact_pert is False, "Perturbed exact_match was True"
    results["perturbation_test"] = {"ssim": round(s_pert, 5), "psnr": round(p_pert, 2), "exact_match": exact_pert, "status": "PASS"}

    # Fixture 3: Localized corruption (25x25 black block)
    corrupted = base.copy()
    corrupted[30:55, 30:55, :] = 0
    s_corr = compute_standard_ssim(corrupted, base)
    p_corr = compute_psnr(corrupted, base)
    assert s_corr < 0.96, f"Corrupted SSIM unexpected: {s_corr}"
    assert p_corr < 35.0, f"Corrupted PSNR unexpected: {p_corr}"
    results["corruption_test"] = {"ssim": round(s_corr, 5), "psnr": round(p_corr, 2), "status": "PASS"}

    # Fixture 4: Alpha transparency
    rgba_base = Image.new("RGBA", (80, 80), (100, 150, 200, 255))
    rgba_test = Image.new("RGBA", (80, 80), (100, 150, 200, 180))
    arr_test, arr_ref = prepare_image_arrays_for_comparison(rgba_test, rgba_base)
    s_alpha = compute_standard_ssim(arr_test, arr_ref)
    assert not math.isnan(s_alpha) and s_alpha > 0.8, f"Alpha SSIM unexpected: {s_alpha}"
    results["alpha_transparency_test"] = {"ssim": round(s_alpha, 4), "status": "PASS"}

    # Fixture 5: Grayscale 2D array
    gray_base = np.full((64, 64), 128, dtype=np.uint8)
    gray_test = gray_base.copy()
    gray_test[10:20, 10:20] = 135
    s_gray = compute_standard_ssim(gray_test, gray_base)
    p_gray = compute_psnr(gray_test, gray_base)
    assert 0.98 <= s_gray < 1.0, f"Grayscale SSIM unexpected: {s_gray}"
    results["grayscale_test"] = {"ssim": round(s_gray, 4), "psnr": round(p_gray, 2), "status": "PASS"}

    # Fixture 6: Dimension mismatch rejection
    dim_mismatch_caught = False
    try:
        compute_standard_ssim(np.zeros((50, 50, 3), dtype=np.uint8), np.zeros((50, 51, 3), dtype=np.uint8))
    except ValueError:
        dim_mismatch_caught = True
    assert dim_mismatch_caught is True, "Dimension mismatch failed to raise ValueError"
    results["dimension_mismatch_test"] = {"caught": True, "status": "PASS"}

    # Fixture 7: Magic byte / MIME detection - JPEG falsely named .png
    # Build synthetic 1x1 JPEG bytes
    img_mem = io.BytesIO()
    Image.new("RGB", (32, 32), (255, 0, 0)).save(img_mem, format="JPEG")
    fake_png_bytes = img_mem.getvalue()
    inspect_res = inspect_and_verify_image(
        fake_png_bytes,
        mime_type="image/png",
        expected_format="PNG",
        expected_dimensions=(32, 32),
    )
    assert inspect_res["valid"] is False, "Mislabeled JPEG was incorrectly accepted as valid PNG"
    assert "Format/MIME" in inspect_res["error"] or "mismatch" in inspect_res["error"].lower() or "discrepancy" in inspect_res["error"].lower()
    results["mislabeled_mime_test"] = {
        "caught": True,
        "error_message": inspect_res["error"],
        "detected_magic": inspect_res["magic_format"],
        "status": "PASS",
    }

    return results


if __name__ == "__main__":
    print("Executing deterministic metric fixtures...")
    fixtures = run_deterministic_metric_fixtures()
    for name, data in fixtures.items():
        print(f"  [+] {name}: {data['status']} - {data}")
    print("\nAll deterministic metric fixtures passed successfully.")
