#!/usr/bin/env python3
"""
Unit test suite for fallback candidate record integrity and validation assertions.

Tests:
1. Valid candidate set passes without error.
2. Missing selection (0 selected candidates) raises AssertionError.
3. Duplicate selection (>1 selected candidates) raises AssertionError.
4. Byte count mismatch (selected candidate bytes != at_auto_bytes) raises AssertionError.
5. Non-minimum candidate selection (selected candidate is not smallest) raises AssertionError.
6. Validation of committed run records in raw_results.json.
"""

import copy
import json
import os
import sys
import unittest

# Ensure bench/ is on import path
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
ROOT_DIR = os.path.dirname(SCRIPT_DIR)
if SCRIPT_DIR not in sys.path:
    sys.path.insert(0, SCRIPT_DIR)

from benchmark_suite import validate_fallback_candidates


class TestFallbackCandidateValidation(unittest.TestCase):
    def setUp(self):
        self.valid_row = {
            "id": "img_19",
            "filename": "alacritty_logo_alpha.png",
            "at_auto_bytes": 3401,
            "fallback_candidates": [
                {
                    "format": "avif",
                    "quality": 90,
                    "bytes": 10477,
                    "ssim": 0.9931,
                    "psnr_db": 49.31,
                    "larger_than_source": True,
                    "selected": False,
                    "role": "nominal_primary",
                },
                {
                    "format": "avif",
                    "quality": 40,
                    "bytes": 4329,
                    "ssim": 0.9807,
                    "psnr_db": 42.59,
                    "larger_than_source": True,
                    "selected": False,
                    "role": "fallback_ladder",
                },
                {
                    "format": "avif",
                    "quality": 20,
                    "bytes": 3401,
                    "ssim": 0.9674,
                    "psnr_db": 38.92,
                    "larger_than_source": False,
                    "selected": True,
                    "role": "fallback_ladder",
                },
            ],
        }

    def test_valid_candidate_set_passes(self):
        # Should complete cleanly with no exceptions
        validate_fallback_candidates(self.valid_row)

    def test_none_or_empty_candidates_pass(self):
        row_none = {"id": "img_01", "fallback_candidates": None, "at_auto_bytes": 1000}
        validate_fallback_candidates(row_none)

        row_empty = {"id": "img_01", "fallback_candidates": [], "at_auto_bytes": 1000}
        validate_fallback_candidates(row_empty)

    def test_missing_selection_fails(self):
        row = copy.deepcopy(self.valid_row)
        for c in row["fallback_candidates"]:
            c["selected"] = False

        with self.assertRaises(AssertionError) as ctx:
            validate_fallback_candidates(row)
        self.assertIn("expected exactly 1 selected candidate", str(ctx.exception))

    def test_duplicate_selection_fails(self):
        row = copy.deepcopy(self.valid_row)
        row["fallback_candidates"][1]["selected"] = True  # Both q=40 and q=20 marked True

        with self.assertRaises(AssertionError) as ctx:
            validate_fallback_candidates(row)
        self.assertIn("expected exactly 1 selected candidate", str(ctx.exception))

    def test_byte_mismatch_fails(self):
        row = copy.deepcopy(self.valid_row)
        row["at_auto_bytes"] = 3500  # selected is 3401, auto is 3500

        with self.assertRaises(AssertionError) as ctx:
            validate_fallback_candidates(row)
        self.assertIn("does not match at_auto_bytes", str(ctx.exception))

    def test_non_minimum_selection_fails(self):
        # A corrupted record where q=40 is selected and at_auto_bytes matches 4329,
        # but q=20 is smaller (3401 bytes).
        row = copy.deepcopy(self.valid_row)
        row["at_auto_bytes"] = 4329
        row["fallback_candidates"][1]["selected"] = True
        row["fallback_candidates"][2]["selected"] = False

        with self.assertRaises(AssertionError) as ctx:
            validate_fallback_candidates(row)
        self.assertIn("is not the smallest evaluated candidate", str(ctx.exception))

    def test_committed_raw_results_pass(self):
        raw_paths = [
            os.path.join(ROOT_DIR, "benchmarks", "raw_results.json"),
            os.path.join(ROOT_DIR, "benchmarks", "runs", "run_20261108_152829", "raw_results.json"),
        ]

        for p in raw_paths:
            self.assertTrue(os.path.exists(p), f"Missing raw_results file: {p}")
            with open(p, "r", encoding="utf-8") as f:
                data = json.load(f)

            for section in ["scenario_9_corpus_summary", "scenario_10_synthetic_summary"]:
                rows = data.get(section, {}).get("rows", [])
                for r in rows:
                    validate_fallback_candidates(r)

        # Historical run_20261108_152829 audit checks (historical q=20 baseline)
        hist_p = os.path.join(ROOT_DIR, "benchmarks", "runs", "run_20261108_152829", "raw_results.json")
        with open(hist_p, "r", encoding="utf-8") as f:
            hist_data = json.load(f)

        calib_rows = hist_data.get("scenario_9_corpus_summary", {}).get("rows", [])
        alacritty = next((r for r in calib_rows if r.get("id") == "img_19"), None)
        self.assertIsNotNone(alacritty)
        self.assertEqual(alacritty["at_auto_bytes"], 3401)
        cands = alacritty["fallback_candidates"]
        self.assertEqual(len(cands), 3)
        selected = [c for c in cands if c.get("selected")]
        self.assertEqual(len(selected), 1)
        self.assertEqual(selected[0]["quality"], 20)
        self.assertEqual(selected[0]["bytes"], 3401)

        synth_rows = hist_data.get("scenario_10_synthetic_summary", {}).get("rows", [])
        checkerboard = next((r for r in synth_rows if r.get("id") == "synth_16"), None)
        self.assertIsNotNone(checkerboard)
        self.assertEqual(checkerboard["at_auto_bytes"], 32357)
        cands_synth = checkerboard["fallback_candidates"]
        self.assertEqual(len(cands_synth), 3)
        selected_synth = [c for c in cands_synth if c.get("selected")]
        self.assertEqual(len(selected_synth), 1)
        self.assertEqual(selected_synth[0]["quality"], 20)
        self.assertEqual(selected_synth[0]["bytes"], 32357)


if __name__ == "__main__":
    unittest.main()
