# GTA 6 Subtitle App — Progress

Mục tiêu: app `.exe` local cho streamer (sangtran) chạy cạnh GTA 6, nghe thoại tiếng Anh và hiện phụ đề tiếng Việt realtime, độ trễ thấp nhất có thể.

Quy tắc làm việc (test -> commit -> push cho từng tính năng, an toàn chống virus): xem `CLAUDE.md`.

## Quyết định đã chốt

- Dịch Anh -> Việt.
- Chạy 100% local, không gửi dữ liệu ra ngoài.
- Rust, một file `.exe` gọn nhẹ (tránh Python/PyInstaller vì nặng và hay bị antivirus báo).
- Không hook/inject vào game. Chỉ bắt audio hệ thống + overlay cửa sổ riêng.

## Kiến trúc (pipeline)

```
Âm thanh game (WASAPI process loopback)
  -> VAD (cắt đoạn có tiếng nói)
  -> ASR streaming (whisper.cpp, GPU)
  -> Dịch Anh->Việt (model local nhẹ)
  -> Overlay trong suốt, click-through, luôn trên cùng
```

Ràng buộc:
- Overlay chỉ hiện khi game chạy **Borderless Windowed**. Exclusive Fullscreen che overlay.
- Streamer dùng OBS: có thể thêm chế độ cửa sổ riêng để OBS "Window Capture" thay vì overlay trên game.
- GPU dùng chung với game + OBS encode -> model phải nhẹ, đo ảnh hưởng FPS.
- Chất lượng dịch Anh->Việt của model nhỏ có thể yếu, phải benchmark ở giai đoạn 3.

## Ngân sách độ trễ (mục tiêu)

| Khâu | Mục tiêu |
|------|----------|
| Bắt audio + chunk | 100-200 ms |
| VAD | < 30 ms |
| ASR (partial) | 200-400 ms |
| Dịch | 50-150 ms |
| Vẽ overlay | < 16 ms |
| **Tổng (chữ đầu hiện)** | **< 700 ms** |

## Giai đoạn (mỗi mục: làm -> test -> commit -> push -> tick)

- [x] **0. Khởi tạo**: progress.md, CLAUDE.md, git init, .gitignore
- [x] **0b. Repo GitHub**: public, giấy phép MIT
- [ ] **1. Capture**: bắt audio tiến trình game, lưu thử, đo độ trễ
- [ ] **2. ASR**: streaming trên audio mẫu, đo latency + độ chính xác
- [ ] **3. Dịch**: benchmark vài model Anh->Việt, chọn cái cân bằng tốc độ/chất lượng
- [ ] **4. Overlay**: cửa sổ trong suốt click-through, chỉnh font/vị trí
- [ ] **5. Ghép pipeline**: đa luồng, hàng đợi nhỏ, partial -> final
- [ ] **6. Tối ưu**: giảm chunk, quantize, đo ảnh hưởng FPS
- [ ] **7. Phát hành**: cài đặt (hotkey, font), build GitHub Actions, checksum SHA256, code signing, `cargo audit`

## Câu hỏi mở

2. GPU của streamer là gì? (quyết định model ASR/dịch)
3. Streamer dùng OBS? Muốn phụ đề hiện trong stream hay chỉ cho streamer đọc?

## Nhật ký

- 2026-11-20: tạo khung dự án, chốt Rust + local + an toàn.
