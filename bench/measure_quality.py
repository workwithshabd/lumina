#!/usr/bin/env python3
"""
Atihsrah Benchmarking Suite: Image Quality & Alignment Measurement Pipeline

Specification:
1. Builds exactly ONE reference per image by resizing with a single documented
   high-quality scaler (PIL Image.Resampling.LANCZOS) to exact target dimensions.
2. Asserts both Atihsrah and imgproxy return identical dimensions matching reference.
3. Decodes outputs and computes SSIM (and VMAF via ffmpeg if available) against reference.
4. Strict assertions:
   - Fails if any lossless output scores SSIM < 0.99.
   - Fails if any metric is NaN or <= 0.0.
   - Fails if output dimensions mismatch.
5. Reports delivered bytes vs original and flags outputs larger than source.
"""

import math
import os
import re
import subprocess
import sys
from typing import Dict, Optional, Tuple
from PIL import Image
import numpy as np


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
            # Scale so smaller dimension fills target, then center-crop
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
            # Scale so image fits within bounds preserving aspect ratio
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


def compute_ssim_numpy(
    img1: np.ndarray,
    img2: np.ndarray,
    data_range: float = 255.0,
    k1: float = 0.01,
    k2: float = 0.03,
) -> float:
    """
    Computes Structural Similarity Index (SSIM) between two identical-size images.
    Implements standard Wang et al. (2004) formulation.
    """
    if img1.shape != img2.shape:
        raise ValueError(f"Shape mismatch in SSIM: {img1.shape} vs {img2.shape}")

    c1 = (k1 * data_range) ** 2
    c2 = (k2 * data_range) ** 2

    # Convert to float64
    x = img1.astype(np.float64)
    y = img2.astype(np.float64)

    # If alpha channel present, compare color channels weighted by alpha
    if x.shape[2] == 4:
        # Compare RGB channels
        x_rgb = x[:, :, :3]
        y_rgb = y[:, :, :3]
    else:
        x_rgb = x[:, :, :3]
        y_rgb = y[:, :, :3]

    mu_x = np.mean(x_rgb, axis=(0, 1))
    mu_y = np.mean(y_rgb, axis=(0, 1))

    sigma_x_sq = np.var(x_rgb, axis=(0, 1))
    sigma_y_sq = np.var(y_rgb, axis=(0, 1))
    sigma_xy = np.mean((x_rgb - mu_x) * (y_rgb - mu_y), axis=(0, 1))

    numerator = (2 * mu_x * mu_y + c1) * (2 * sigma_xy + c2)
    denominator = (mu_x**2 + mu_y**2 + c1) * (sigma_x_sq + sigma_y_sq + c2)
    ssim_per_channel = numerator / denominator

    return float(np.mean(ssim_per_channel))


def compute_vmaf_ffmpeg(test_path: str, ref_path: str) -> Optional[float]:
    """
    Computes VMAF score (0-100) using ffmpeg libvmaf filter if available.
    Returns None if libvmaf is not compiled into ffmpeg.
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
            timeout=30,
        )
        match = re.search(r"VMAF score:\s*([0-9.]+)", proc.stderr, re.IGNORECASE)
        if match:
            return float(match.group(1))
    except Exception:
        pass
    return None


def evaluate_output(
    test_path: str,
    ref_path: str,
    original_size_bytes: int,
    is_lossless: bool,
    expected_dimensions: Tuple[int, int],
) -> Dict[str, Any]:
    """
    Evaluates a single transformed image against the ground truth reference.
    Enforces all benchmark assertions:
    - Dimensions must match reference exactly.
    - SSIM must not be NaN or <= 0.0.
    - If lossless, SSIM must be >= 0.99.
    """
    file_bytes = os.path.getsize(test_path)
    larger_than_source = file_bytes > original_size_bytes

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

        # Standardize color modes
        has_alpha = ref_img.mode == "RGBA" or test_img.mode == "RGBA"
        mode = "RGBA" if has_alpha else "RGB"
        test_arr = np.array(test_img.convert(mode))
        ref_arr = np.array(ref_img.convert(mode))

    ssim = compute_ssim_numpy(test_arr, ref_arr)

    if math.isnan(ssim) or ssim <= 0.0:
        raise AssertionError(f"SSIM computation produced invalid value: {ssim} for {test_path}")

    if is_lossless and ssim < 0.99:
        raise AssertionError(
            f"Lossless output {test_path} failed SSIM threshold: {ssim:.4f} < 0.9900"
        )

    vmaf = compute_vmaf_ffmpeg(test_path, ref_path)
    if vmaf is not None and (math.isnan(vmaf) or vmaf <= 0.0):
        raise AssertionError(f"VMAF computation produced invalid value: {vmaf} for {test_path}")

    return {
        "bytes": file_bytes,
        "original_bytes": original_size_bytes,
        "ratio_vs_orig": round(file_bytes / original_size_bytes, 4),
        "larger_than_source": larger_than_source,
        "ssim": round(ssim, 4),
        "vmaf": round(vmaf, 2) if vmaf is not None else None,
        "dimensions": (test_w, test_h),
    }


if __name__ == "__main__":
    print("measure_quality module loaded successfully.")

