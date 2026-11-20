# GTA 6 Subtitle App — Progress

Mục tiêu: app `.exe` local cho streamer (sangtran) chạy cạnh GTA 6. App đọc chữ tiếng Anh trên màn hình game (menu, hội thoại, phụ đề) và hiện bản dịch tiếng Việt ngay bên trên, độ trễ thấp nhất có thể. Viewer cũng thấy bản dịch trên stream OBS.

Quy tắc làm việc (test -> commit -> push cho từng tính năng, an toàn chống virus): xem `CLAUDE.md`.

## Quyết định đã chốt

- Dịch Anh -> Việt, **dịch chữ trên màn hình** (OCR), không dịch giọng nói. Ví dụ mẫu: "Buy a new outfit" -> "Mua bộ đồ mới" hiện phía trên dòng gốc.
- Chạy 100% local, không gửi dữ liệu ra ngoài.
- Rust, một file `.exe` gọn nhẹ.
- Không hook/inject vào game. Chỉ chụp màn hình bằng API của Windows + overlay cửa sổ riêng.
- Phải chạy được trên máy không rõ cấu hình: ưu tiên CPU, GPU chỉ là tăng tốc tùy chọn.
- Hiện bản dịch lên stream: OBS bắt cửa sổ overlay của app bằng Window Capture.

## Kiến trúc (pipeline)

```
Chụp vùng chữ của cửa sổ game (Windows Graphics Capture)
  -> Phát hiện thay đổi (bỏ qua khung hình không đổi)
  -> OCR tiếng Anh (Windows.Media.Ocr có sẵn, chạy CPU, không cần tải model)
  -> Cache bản dịch (chuỗi lặp lại như menu: trả ngay, ~0 ms)
  -> Dịch Anh->Việt (model local nhỏ, CPU)
  -> Overlay trong suốt, hiện bản dịch ngay trên chữ gốc
```

Vì sao OCR thay vì nghe giọng: GTA đã có sẵn phụ đề/menu tiếng Anh trên màn hình, đọc chữ chính xác hơn nhận dạng giọng (tên riêng, tiếng lóng), nhẹ hơn nhiều và không cần GPU.

Ràng buộc:
- Game phải chạy **Borderless Windowed** (Exclusive Fullscreen không chụp/overlay được tốt).
- Overlay không được đặt cờ "loại khỏi capture" để OBS vẫn bắt được.
- Chỉ OCR vùng cần thiết (người dùng chọn vùng) để giảm tải CPU và tăng tốc.
- Chất lượng dịch Anh->Việt của model nhỏ phải benchmark kỹ (giai đoạn 3); thêm từ điển tùy chỉnh cho tên riêng GTA (Los Santos, ...).

## Ngân sách độ trễ (mục tiêu)

| Khâu | Mục tiêu |
|------|----------|
| Chụp vùng + phát hiện đổi | < 20 ms |
| OCR | 30-100 ms |
| Dịch (cache miss) | 50-150 ms |
| Dịch (cache hit) | ~0 ms |
| Vẽ overlay | < 16 ms |
| **Tổng (chữ mới)** | **< 300 ms** |

## Giai đoạn (mỗi mục: làm -> test -> commit -> push -> tick)

- [x] **0. Khởi tạo**: progress.md, CLAUDE.md, git init, .gitignore, repo public MIT
- [ ] **1. Capture**: chụp cửa sổ/vùng game bằng Windows Graphics Capture, đo độ trễ, đo tải CPU
- [ ] **2. OCR**: đọc chữ tiếng Anh từ ảnh mẫu GTA, đo tốc độ + độ chính xác
- [ ] **3. Dịch**: benchmark vài model Anh->Việt chạy CPU, chọn cái cân bằng tốc độ/chất lượng; thêm cache + từ điển
- [ ] **4. Overlay**: cửa sổ trong suốt click-through, đặt bản dịch trên chữ gốc, chỉnh font/vị trí/vùng quét
- [ ] **5. Ghép pipeline**: đa luồng, bỏ qua khung không đổi, chống nhấp nháy khi chữ đổi
- [ ] **6. OBS**: kiểm tra Window Capture bắt được overlay, hướng dẫn cài cho streamer
- [ ] **7. Tối ưu**: đo ảnh hưởng FPS game, giảm tải CPU
- [ ] **8. Phát hành**: cài đặt (hotkey, font, vùng), build GitHub Actions, checksum SHA256, code signing, `cargo audit`

## Câu hỏi mở

1. Streamer muốn dịch những loại chữ nào: chỉ phụ đề hội thoại, hay cả menu/nhiệm vụ/biển hiệu?
2. Cấu hình máy streamer (CPU/RAM; GPU chỉ cần nếu muốn tăng tốc), có thể để sau.
3. Có ảnh chụp màn hình GTA mẫu (phụ đề, menu) để test OCR không? Chưa có GTA 6 PC thì dùng GTA 5 để test.

## Nhật ký

- 2026-11-20: tạo khung dự án, chốt Rust + local + an toàn.
- 2026-11-20: đổi hướng từ nghe giọng nói sang OCR chữ trên màn hình theo ví dụ của streamer.
