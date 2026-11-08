use image::{ImageBuffer, Rgb, Rgba};
use std::f32::consts::PI;
use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = Path::new("benchmarks/corpus");
    fs::create_dir_all(out_dir)?;

    println!(
        "Generating 20-image benchmark corpus in {}...",
        out_dir.display()
    );

    // 1. Natural photo: Landscape (1920x1080)
    {
        let width = 1920;
        let height = 1080;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            let ny = y as f32 / height as f32;
            for x in 0..width {
                let nx = x as f32 / width as f32;
                if ny < 0.5 {
                    // Sky gradient with soft clouds
                    let cloud = (nx * 12.0).sin() * (ny * 16.0).cos() * 0.15;
                    let r = ((0.3 + cloud) * 255.0).clamp(0.0, 255.0) as u8;
                    let g = ((0.55 + cloud) * 255.0).clamp(0.0, 255.0) as u8;
                    let b = ((0.85 + cloud * 0.5) * 255.0).clamp(0.0, 255.0) as u8;
                    img.put_pixel(x, y, Rgb([r, g, b]));
                } else {
                    // Hills and vegetation
                    let hill = ((nx * 6.0).sin() * 0.08) + ((nx * 20.0).sin() * 0.03);
                    let hill_y = 0.5 + hill;
                    if ny < hill_y {
                        let r = 70;
                        let g = 130;
                        let b = 180;
                        img.put_pixel(x, y, Rgb([r, g, b]));
                    } else {
                        let grass_noise =
                            ((x as f32 * 0.4).sin() * (y as f32 * 0.4).cos() * 0.2).abs();
                        let r = ((0.15 + grass_noise * 0.1) * 255.0) as u8;
                        let g = ((0.45 + grass_noise * 0.3) * 255.0) as u8;
                        let b = ((0.12 + grass_noise * 0.05) * 255.0) as u8;
                        img.put_pixel(x, y, Rgb([r, g, b]));
                    }
                }
            }
        }
        img.save(out_dir.join("01_landscape.png"))?;
    }

    // 2. Natural photo: Portrait / Skin Tones (1200x1600)
    {
        let width = 1200;
        let height = 1600;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        let cx = width as f32 / 2.0;
        let cy = height as f32 * 0.45;
        for y in 0..height {
            for x in 0..width {
                let dx = (x as f32 - cx) / (width as f32 * 0.35);
                let dy = (y as f32 - cy) / (height as f32 * 0.35);
                let d = (dx * dx + dy * dy).sqrt();
                if d < 1.0 {
                    // Warm skin tones with subtle shading
                    let shade = 1.0 - (d * 0.4);
                    let r = (235.0 * shade) as u8;
                    let g = (185.0 * shade) as u8;
                    let b = (155.0 * shade) as u8;
                    img.put_pixel(x, y, Rgb([r, g, b]));
                } else {
                    // Studio portrait background
                    let bg = (0.2 + (y as f32 / height as f32) * 0.2) * 255.0;
                    img.put_pixel(x, y, Rgb([bg as u8, bg as u8, (bg * 1.1) as u8]));
                }
            }
        }
        img.save(out_dir.join("02_portrait.jpg"))?;
    }

    // 3. High-Frequency Macro Texture (1400x1400)
    {
        let width = 1400;
        let height = 1400;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let s1 = ((x as f32 * 0.15).sin() * 0.5 + 0.5) * 120.0;
                let s2 = ((y as f32 * 0.15).cos() * 0.5 + 0.5) * 120.0;
                let s3 = (((x + y) as f32 * 0.08).sin() * 0.5 + 0.5) * 15.0;
                let v = (s1 + s2 + s3) as u8;
                img.put_pixel(
                    x,
                    y,
                    Rgb([v, (v as f32 * 0.9) as u8, (v as f32 * 0.75) as u8]),
                );
            }
        }
        img.save(out_dir.join("03_macro_texture.png"))?;
    }

    // 4. Night Skyline / Low-Light Noise (1920x1080)
    {
        let width = 1920;
        let height = 1080;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            for x in 0..width {
                if y < 700 {
                    // Dark night sky with tiny stars
                    let mut r = 8u8;
                    let mut g = 10u8;
                    let mut b = 25u8;
                    if (x * 37 + y * 91) % 499 == 0 {
                        r = 220;
                        g = 230;
                        b = 255;
                    }
                    img.put_pixel(x, y, Rgb([r, g, b]));
                } else {
                    // Skyline building blocks and neon windows
                    let b_id = x / 60;
                    let win_x = (x % 60) / 10;
                    let win_y = (y % 40) / 10;
                    let is_window = (win_x == 2 || win_x == 4) && (win_y == 1 || win_y == 2);
                    if is_window && (b_id * 13 + win_x * 7 + win_y * 11) % 3 != 0 {
                        img.put_pixel(x, y, Rgb([255, 220, 110]));
                    } else {
                        img.put_pixel(x, y, Rgb([18, 20, 30]));
                    }
                }
            }
        }
        img.save(out_dir.join("04_night_skyline.jpg"))?;
    }

    // 5. Architecture Geometric Lines (1600x1200)
    {
        let width = 1600;
        let height = 1200;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let is_grid = (x % 50 < 3) || (y % 50 < 3);
                let diag = (x + y * 2) % 150 < 4;
                if is_grid || diag {
                    img.put_pixel(x, y, Rgb([230, 230, 235]));
                } else {
                    let base = 70 + ((x % 50) as u8) + ((y % 50) as u8);
                    img.put_pixel(x, y, Rgb([base, base + 10, base + 25]));
                }
            }
        }
        img.save(out_dir.join("05_architecture.png"))?;
    }

    // 6. Digital Art: Flat Vector (1200x1200)
    {
        let width = 1200;
        let height = 1200;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let cx = x as f32 - 600.0;
                let cy = y as f32 - 600.0;
                let dist = (cx * cx + cy * cy).sqrt();
                if dist < 200.0 {
                    img.put_pixel(x, y, Rgb([240, 80, 70])); // red circle
                } else if dist < 380.0 {
                    img.put_pixel(x, y, Rgb([250, 190, 40])); // yellow ring
                } else if x < 600 && y < 600 {
                    img.put_pixel(x, y, Rgb([40, 140, 220])); // blue quadrant
                } else if x >= 600 && y >= 600 {
                    img.put_pixel(x, y, Rgb([50, 180, 120])); // green quadrant
                } else {
                    img.put_pixel(x, y, Rgb([245, 245, 245])); // off-white
                }
            }
        }
        img.save(out_dir.join("06_vector_flat.png"))?;
    }

    // 7. Smooth Color Gradient (1920x1080)
    {
        let width = 1920;
        let height = 1080;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            let ty = y as f32 / height as f32;
            for x in 0..width {
                let tx = x as f32 / width as f32;
                let r = (tx * 255.0) as u8;
                let g = (ty * 255.0) as u8;
                let b = (((1.0 - tx) * 0.5 + (1.0 - ty) * 0.5) * 255.0) as u8;
                img.put_pixel(x, y, Rgb([r, g, b]));
            }
        }
        img.save(out_dir.join("07_smooth_gradient.png"))?;
    }

    // 8. Typography & Poster Graphic (1200x1600)
    {
        let width = 1200;
        let height = 1600;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let stripe = ((x + y) / 40) % 2 == 0;
                if y > 400 && y < 1200 && x > 200 && x < 1000 {
                    // Center poster box with high-contrast geometric block letters
                    let bx = x - 200;
                    let by = y - 400;
                    let block = ((bx / 80) + (by / 80)) % 2 == 0;
                    if block {
                        img.put_pixel(x, y, Rgb([20, 20, 20]));
                    } else {
                        img.put_pixel(x, y, Rgb([255, 60, 40]));
                    }
                } else if stripe {
                    img.put_pixel(x, y, Rgb([240, 240, 240]));
                } else {
                    img.put_pixel(x, y, Rgb([200, 200, 205]));
                }
            }
        }
        img.save(out_dir.join("08_typography_poster.png"))?;
    }

    // 9. UI Screenshot: Desktop (1920x1080)
    {
        let width = 1920;
        let height = 1080;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            for x in 0..width {
                if x < 260 {
                    // Sidebar
                    img.put_pixel(x, y, Rgb([24, 26, 32]));
                } else if y < 64 {
                    // Top header
                    img.put_pixel(x, y, Rgb([30, 32, 40]));
                } else {
                    // Content area with tables and cards
                    let row = (y - 64) / 48;
                    let is_divider = (y - 64) % 48 == 0;
                    if is_divider {
                        img.put_pixel(x, y, Rgb([45, 48, 60]));
                    } else if row % 2 == 0 {
                        img.put_pixel(x, y, Rgb([16, 18, 22]));
                    } else {
                        img.put_pixel(x, y, Rgb([20, 22, 28]));
                    }
                }
            }
        }
        img.save(out_dir.join("09_ui_desktop.png"))?;
    }

    // 10. UI Screenshot: Mobile (750x1334)
    {
        let width = 750;
        let height = 1334;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            for x in 0..width {
                if y < 88 {
                    // Status bar
                    img.put_pixel(x, y, Rgb([255, 255, 255]));
                } else if y > 1250 {
                    // Bottom tab bar
                    img.put_pixel(x, y, Rgb([248, 248, 248]));
                } else {
                    // Feed cards
                    let card_idx = (y - 88) / 280;
                    let card_y = (y - 88) % 280;
                    if x > 32 && x < 718 && card_y > 20 && card_y < 260 {
                        let inner_r = if card_idx % 2 == 0 { 255 } else { 245 };
                        img.put_pixel(x, y, Rgb([inner_r, 255, 255]));
                    } else {
                        img.put_pixel(x, y, Rgb([240, 242, 245]));
                    }
                }
            }
        }
        img.save(out_dir.join("10_ui_mobile.png"))?;
    }

    // 11. Document: Scanned Page / Text (1650x2338)
    {
        let width = 1650;
        let height = 2338;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let paper = 248 + ((x * 17 + y * 31) % 5) as u8;
                // Margins: 150px left/right, 200px top/bottom
                if x > 150 && x < 1500 && y > 200 && y < 2100 {
                    let line = (y - 200) / 36;
                    let line_y = (y - 200) % 36;
                    let is_text_row = (8..=20).contains(&line_y);
                    let is_word_space = ((x * 13 + line * 37) % 120) < 25;
                    if is_text_row && !is_word_space && line % 3 != 2 {
                        img.put_pixel(x, y, Rgb([25, 25, 28]));
                    } else {
                        img.put_pixel(x, y, Rgb([paper, paper, paper - 3]));
                    }
                } else {
                    img.put_pixel(x, y, Rgb([paper, paper, paper - 3]));
                }
            }
        }
        img.save(out_dir.join("11_document_text.png"))?;
    }

    // 12. Transparent RGBA Icon with Alpha (512x512)
    {
        let width = 512;
        let height = 512;
        let mut img = ImageBuffer::<Rgba<u8>, _>::new(width, height);
        let cx = 256.0;
        let cy = 256.0;
        for y in 0..height {
            for x in 0..width {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist < 220.0 {
                    let a = ((220.0 - dist).min(1.0) * 255.0) as u8;
                    img.put_pixel(x, y, Rgba([40, 160, 240, a]));
                } else {
                    img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                }
            }
        }
        img.save(out_dir.join("12_transparent_icon.png"))?;
    }

    // 13. Transparent RGBA with Soft Drop-Shadow (800x800)
    {
        let width = 800;
        let height = 800;
        let mut img = ImageBuffer::<Rgba<u8>, _>::new(width, height);
        for y in 0..height {
            for x in 0..width {
                // Main card: 200..600, 200..600
                if (200..=600).contains(&x) && (200..=600).contains(&y) {
                    img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
                } else {
                    // Soft shadow around card
                    let dx = if x < 200 {
                        200 - x
                    } else {
                        x.saturating_sub(600)
                    };
                    let dy = if y < 200 {
                        200 - y
                    } else {
                        y.saturating_sub(600)
                    };
                    let dist = ((dx * dx + dy * dy) as f32).sqrt();
                    if dist < 60.0 {
                        let alpha = ((1.0 - dist / 60.0) * 120.0) as u8;
                        img.put_pixel(x, y, Rgba([0, 0, 0, alpha]));
                    } else {
                        img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                    }
                }
            }
        }
        img.save(out_dir.join("13_transparent_shadow.png"))?;
    }

    // 14. High Dynamic Range / Contrast (1920x1080)
    {
        let width = 1920;
        let height = 1080;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let dx = x as f32 - 960.0;
                let dy = y as f32 - 540.0;
                let dist = (dx * dx + dy * dy).sqrt();
                // Intense bright spotlight falling to pure black
                let val = (1000.0 / (dist + 5.0)).powf(1.8).clamp(0.0, 255.0) as u8;
                img.put_pixel(x, y, Rgb([val, val, val]));
            }
        }
        img.save(out_dir.join("14_hdr_highcontrast.jpg"))?;
    }

    // 15. Low Contrast / Fog / Muted (1600x1200)
    {
        let width = 1600;
        let height = 1200;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            let ty = y as f32 / height as f32;
            for x in 0..width {
                let tx = x as f32 / width as f32;
                // Values tightly clustered between 120 and 160
                let r = (130.0 + tx * 20.0 + ty * 10.0) as u8;
                let g = (135.0 + ty * 25.0) as u8;
                let b = (140.0 + (1.0 - tx) * 20.0) as u8;
                img.put_pixel(x, y, Rgb([r, g, b]));
            }
        }
        img.save(out_dir.join("15_lowcontrast_fog.jpg"))?;
    }

    // 16. Checkerboard Pattern (1024x1024)
    {
        let width = 1024;
        let height = 1024;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let check = ((x / 16) + (y / 16)) % 2 == 0;
                let v = if check { 240 } else { 20 };
                img.put_pixel(x, y, Rgb([v, v, v]));
            }
        }
        img.save(out_dir.join("16_pattern_checkerboard.png"))?;
    }

    // 17. High Noise / Synthetic Film Grain (1280x720)
    {
        let width = 1280;
        let height = 720;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            for x in 0..width {
                // Pseudo-random noise centered at 128
                let noise = ((x * 48271 + y * 16807 + 97) % 81) as i16 - 40;
                let v = (128 + noise).clamp(0, 255) as u8;
                img.put_pixel(x, y, Rgb([v, v, v]));
            }
        }
        img.save(out_dir.join("17_noise_grain.png"))?;
    }

    // 18. Large 4K UHD Photo (3840x2160)
    {
        let width = 3840;
        let height = 2160;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            let ny = y as f32 / height as f32;
            for x in 0..width {
                let nx = x as f32 / width as f32;
                let pattern = ((nx * 40.0).sin() * (ny * 40.0).cos() * 0.3 + 0.5) * 200.0;
                let r = (pattern * 0.8 + nx * 50.0) as u8;
                let g = (pattern * 1.1 + ny * 30.0).min(255.0) as u8;
                let b = (pattern * 0.9) as u8;
                img.put_pixel(x, y, Rgb([r, g, b]));
            }
        }
        img.save(out_dir.join("18_photo_4k_large.jpg"))?;
    }

    // 19. Medium Web Hero Image (1200x800)
    {
        let width = 1200;
        let height = 800;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            let ny = y as f32 / height as f32;
            for x in 0..width {
                let nx = x as f32 / width as f32;
                let wave = ((nx * 8.0 * PI).sin() * 0.2 + 0.5) * 255.0;
                let r = (wave * (1.0 - ny)) as u8;
                let g = (120.0 + ny * 100.0) as u8;
                let b = (200.0 * nx) as u8;
                img.put_pixel(x, y, Rgb([r, g, b]));
            }
        }
        img.save(out_dir.join("19_photo_medium.jpg"))?;
    }

    // 20. Small Thumbnail Source (400x300)
    {
        let width = 400;
        let height = 300;
        let mut img = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let v = ((x ^ y) % 256) as u8;
                img.put_pixel(x, y, Rgb([v, 150, 255 - v]));
            }
        }
        img.save(out_dir.join("20_thumbnail_small.png"))?;
    }

    println!("Successfully generated all 20 benchmark corpus images!");
    Ok(())
}
