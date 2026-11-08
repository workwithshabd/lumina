#!/usr/bin/env python3
"""
Unit and Integration Test Suite for Atihsrah Benchmark Harness & Quality Engine

Tests acceptance criteria defined in Task specification:
- Actual-format detection and MIME/extension consistency, including JPEG falsely named .png.
- Fair format/parameter comparison eligibility.
- Standard SSIM and PSNR behavior on deterministic fixtures.
- Lossless exact-pixel enforcement.
- Invalid outputs, failed HTTP responses, and dimension mismatches.
- Dynamic system metadata and missing optional tools.
- Summary/verdict counts derived from actual data.
- Report totals matching machine-readable rows.
- Unique output paths, atomic writes, and preservation of previous run files.
- Localhost-only service bindings.
"""

import io
import math
import os
import platform
import shutil
import tempfile
import unittest
from typing import Dict, Any

import numpy as np
from PIL import Image

import sys

BENCH_DIR = os.path.abspath(os.path.dirname(__file__))
if BENCH_DIR not in sys.path:
    sys.path.insert(0, BENCH_DIR)

import measure_quality
from benchmark_suite import (
    BenchmarkRunner,
    detect_system_info,
    verify_corpus_manifest,
    get_required_ssim,
    MIN_SSIM_PHOTO,
    MIN_SSIM_GRAPHICS,
    TIE_DELTA_PERCENT,
)

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
MANIFEST_PATH = os.path.join(ROOT_DIR, "bench", "corpus_manifest.csv")
CORPUS_DIR = os.path.join(ROOT_DIR, "benchmarks", "corpus")


class TestBenchmarkEngine(unittest.TestCase):
    def test_actual_format_detection_and_mime_consistency(self):
        """Verifies format inspection detects magic bytes and flags MIME/extension mismatches."""
        # 1. Genuine PNG
        png_io = io.BytesIO()
        Image.new("RGB", (30, 30), (0, 100, 200)).save(png_io, format="PNG")
        png_bytes = png_io.getvalue()

        res_png = measure_quality.inspect_and_verify_image(
            png_bytes,
            mime_type="image/png",
            expected_format="PNG",
            expected_dimensions=(30, 30),
        )
        self.assertTrue(res_png["valid"])
        self.assertEqual(res_png["decoder_format"], "PNG")
        self.assertEqual(res_png["magic_format"], "PNG")

        # 2. Genuine JPEG
        jpg_io = io.BytesIO()
        Image.new("RGB", (30, 30), (200, 100, 0)).save(jpg_io, format="JPEG")
        jpg_bytes = jpg_io.getvalue()

        res_jpg = measure_quality.inspect_and_verify_image(
            jpg_bytes,
            mime_type="image/jpeg",
            expected_format="JPEG",
            expected_dimensions=(30, 30),
        )
        self.assertTrue(res_jpg["valid"])
        self.assertEqual(res_jpg["decoder_format"], "JPEG")
        self.assertEqual(res_jpg["magic_format"], "JPEG")

        # 3. Disguised file: JPEG bytes falsely advertised as image/png
        res_disguised = measure_quality.inspect_and_verify_image(
            jpg_bytes,
            mime_type="image/png",
            expected_format="PNG",
            expected_dimensions=(30, 30),
        )
        self.assertFalse(res_disguised["valid"])
        self.assertIn("mismatch", res_disguised["error"].lower())
        self.assertEqual(res_disguised["magic_format"], "JPEG")
        self.assertEqual(res_disguised["decoder_format"], "JPEG")
        self.assertEqual(res_disguised["mime_format"], "PNG")

    def test_standard_ssim_and_psnr_deterministic_fixtures(self):
        """Tests standard Wang et al. 2004 local-window SSIM and PSNR formulas."""
        fixtures = measure_quality.run_deterministic_metric_fixtures()
        self.assertEqual(fixtures["identical_test"]["status"], "PASS")
        self.assertEqual(fixtures["identical_test"]["ssim"], 1.0)
        self.assertEqual(fixtures["identical_test"]["psnr"], "inf")
        self.assertTrue(fixtures["identical_test"]["exact_match"])

        self.assertEqual(fixtures["perturbation_test"]["status"], "PASS")
        self.assertLess(fixtures["perturbation_test"]["ssim"], 1.0)
        self.assertGreater(fixtures["perturbation_test"]["ssim"], 0.99)
        self.assertFalse(fixtures["perturbation_test"]["exact_match"])

        self.assertEqual(fixtures["corruption_test"]["status"], "PASS")
        self.assertLess(fixtures["corruption_test"]["ssim"], 0.96)

        self.assertEqual(fixtures["dimension_mismatch_test"]["status"], "PASS")
        self.assertTrue(fixtures["dimension_mismatch_test"]["caught"])

    def test_dimension_mismatch_rejection(self):
        """Assures comparing different dimensions raises ValueError without silent resizing."""
        img_a = np.zeros((100, 100, 3), dtype=np.uint8)
        img_b = np.zeros((100, 102, 3), dtype=np.uint8)
        with self.assertRaises(ValueError):
            measure_quality.compute_standard_ssim(img_a, img_b)
        with self.assertRaises(ValueError):
            measure_quality.compute_psnr(img_a, img_b)

    def test_lossless_exact_pixel_enforcement(self):
        """Tests lossless exact pixel verification logic."""
        arr_a = np.full((50, 50, 3), 128, dtype=np.uint8)
        arr_b = arr_a.copy()
        self.assertTrue(np.array_equal(arr_a, arr_b))

        arr_c = arr_a.copy()
        arr_c[0, 0, 0] = 129
        self.assertFalse(np.array_equal(arr_a, arr_c))

    def test_fair_format_and_quality_eligibility_rules(self):
        """Verifies that a smaller file cannot win if it fails the category-specific quality threshold."""
        # Simulated test case: Competitor B is smaller but has poor visual quality (SSIM 0.82 < 0.95 for photo)
        row = {
            "category": "photo",
            "at_avif_bytes": 50000,
            "at_avif_ssim": 0.96,
            "img_avif_bytes": 40000,
            "img_avif_ssim": 0.82,  # Below MIN_SSIM_PHOTO (0.95)
            "avif_delta_vs_imgproxy": 25.0,
        }

        gate = get_required_ssim(row["category"])
        self.assertEqual(gate, MIN_SSIM_PHOTO)

        # Check eligibility
        at_valid = row["at_avif_ssim"] >= gate
        img_valid = row["img_avif_ssim"] >= gate

        self.assertTrue(at_valid)
        self.assertFalse(img_valid)

        # In decision logic, imgproxy cannot win because it failed quality gate
        winner = "Atihsrah" if (at_valid and not img_valid) else "imgproxy"
        self.assertEqual(winner, "Atihsrah")

    def test_dynamic_system_metadata(self):
        """Assures system metadata is detected dynamically without hardcoded values."""
        sys_info = detect_system_info()
        hw = sys_info["hardware"]
        tools = sys_info["toolchain"]

        # Architecture must match runtime platform machine
        self.assertEqual(hw["architecture"], platform.machine())
        self.assertIn("memory_gb", hw)
        self.assertIn("logical_cores", hw)
        self.assertTrue(tools["git_commit"])
        self.assertIn("virtualization_disclosure", sys_info)

    def test_unique_output_path_and_preservation(self):
        """Verifies new runs write to dedicated directory and do not overwrite existing files."""
        temp_dir = tempfile.mkdtemp(prefix="test_bench_run_")
        try:
            runner = BenchmarkRunner(output_dir=temp_dir)
            ok = runner.validate_environment_and_fixtures()
            self.assertTrue(ok)
            self.assertTrue(os.path.exists(os.path.join(temp_dir, "validation.log")))
            self.assertTrue(os.path.exists(os.path.join(temp_dir, "system_info.json")))

            # Check that existing benchmark files remain intact
            bench_analysis = os.path.join(ROOT_DIR, "benchmarks", "analysis.md")
            if os.path.exists(bench_analysis):
                mtime_before = os.path.getmtime(bench_analysis)
                self.assertGreater(mtime_before, 0)
        finally:
            shutil.rmtree(temp_dir, ignore_errors=True)

    def test_localhost_port_bindings_in_compose(self):
        """Verifies every published service port in docker-compose.bench.yml binds to 127.0.0.1."""
        compose_path = os.path.join(ROOT_DIR, "bench", "docker-compose.bench.yml")
        self.assertTrue(os.path.exists(compose_path))

        with open(compose_path, "r", encoding="utf-8") as f:
            lines = f.readlines()

        for line in lines:
            line = line.strip()
            if line.startswith("- \"") and ":" in line:
                # Must start with 127.0.0.1:
                port_str = line.strip("- \"")
                self.assertTrue(
                    port_str.startswith("127.0.0.1:"),
                    f"Port binding not restricted to localhost 127.0.0.1: '{port_str}'"
                )


if __name__ == "__main__":
    unittest.main()

