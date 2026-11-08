#!/usr/bin/env python3
"""
Independent Quality Verification Script
Reproduces SSIM and PSNR calculations between Atihsrah outputs and ground-truth Lanczos references.
Strictly verifies that recomputed values match the reported metrics at displayed precision.
"""
import os
import sys
import json
import math
import numpy as np
from PIL import Image
from skimage.metrics import structural_similarity as compute_ssim

ARTIFACTS_DIR = os.environ.get("ARTIFACTS_DIR", "monitoring/evaluation_IMG_0172")
METRICS_JSON = os.environ.get("METRICS_JSON", "monitoring/metrics_IMG_0172.json")

def calculate_psnr(img1_np, img2_np):
    mse = np.mean((img1_np.astype(np.float64) - img2_np.astype(np.float64)) ** 2)
    if mse == 0:
        return float("inf")
    return 10.0 * math.log10((255.0 ** 2) / mse)

def main():
    if not os.path.exists(METRICS_JSON):
        print(f"Error: Metrics file not found at {METRICS_JSON}", file=sys.stderr)
        sys.exit(1)

    with open(METRICS_JSON) as f:
        data = json.load(f)

    scenarios = data.get("transformationQualityResults", [])
    if not scenarios or not isinstance(scenarios, list) or len(scenarios) == 0:
        print(f"Error: No transformation scenarios found in {METRICS_JSON}", file=sys.stderr)
        sys.exit(1)

    print("=" * 90)
    print("        Independent Quality Verification: SSIM & PSNR for IMG_0172")
    print("=" * 90)
    header = f"{'Test Scenario':<32} | {'SSIM (Rep / Recomp)':<22} | {'PSNR (Rep / Recomp)':<22} | {'Status'}"
    print(header)
    print("-" * 90)

    verified_count = 0
    all_matched = True

    for item in scenarios:
        out_path = os.path.join(ARTIFACTS_DIR, item["outputFilename"])
        ref_path = os.path.join(ARTIFACTS_DIR, item["referenceFilename"])

        if not os.path.exists(out_path):
            print(f"Error: Output image missing: {out_path}", file=sys.stderr)
            all_matched = False
            continue
        if not os.path.exists(ref_path):
            print(f"Error: Reference image missing: {ref_path}", file=sys.stderr)
            all_matched = False
            continue

        out_img = Image.open(out_path).convert("RGB")
        ref_img = Image.open(ref_path).convert("RGB")

        out_np = np.array(out_img)
        ref_np = np.array(ref_img)

        # 1. Compute SSIM (Wang et al. 2004, 11x11 Gaussian window)
        raw_ssim = compute_ssim(
            ref_np, out_np,
            channel_axis=2, win_size=11,
            gaussian_weights=True, sigma=1.5,
            data_range=255.0
        )
        ssim_calc_str = f"{float(raw_ssim):.4f}"
        ssim_rep_str = f"{float(item['ssim']):.4f}"
        ssim_match = (ssim_calc_str == ssim_rep_str)

        # 2. Compute PSNR strictly formatted to 2 decimal places
        raw_psnr = calculate_psnr(ref_np, out_np)
        if item["psnrDb"] == "inf":
            psnr_calc_str = "inf" if math.isinf(raw_psnr) else f"{raw_psnr:.2f}"
            psnr_rep_str = "inf"
            psnr_match = math.isinf(raw_psnr)
        else:
            psnr_calc_str = f"{raw_psnr:.2f}"
            psnr_rep_str = f"{float(item['psnrDb']):.2f}"
            psnr_match = (psnr_calc_str == psnr_rep_str)

        if ssim_match and psnr_match:
            status = "MATCH (at displayed precision)"
            verified_count += 1
        else:
            status = "MISMATCH"
            all_matched = False

        ssim_display = f"{ssim_rep_str} / {ssim_calc_str}"
        psnr_display = f"{psnr_rep_str} dB / {psnr_calc_str} dB"
        print(f"{item['name']:<32} | {ssim_display:<22} | {psnr_display:<22} | {status}")

    print("=" * 90)
    total_scenarios = len(scenarios)
    if all_matched and verified_count == total_scenarios and total_scenarios > 0:
        print(f"✓ All {verified_count} scenario outputs match reported SSIM (4 decimals) and PSNR (2 decimals) at displayed precision.")
        sys.exit(0)
    else:
        print(f"⚠ Verification failed: {verified_count}/{total_scenarios} scenarios matched at displayed precision.", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
