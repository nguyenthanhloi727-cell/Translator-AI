# GTA 6 Subtitle App — Progress

Mục tiêu: app `.exe` local cho streamer (sangtran). GTA 6 chạy trên **PS5**, PS5 nối vào PC (capture card hoặc PS Remote Play), hình game hiện trong một cửa sổ trên PC. App đọc phụ đề tiếng Anh trong hình đó và hiện bản dịch tiếng Việt, độ trễ thấp nhất có thể. Viewer cũng thấy bản dịch trên stream OBS.

Quy tắc làm việc (test -> commit -> push cho từng tính năng, an toàn chống virus): xem `CLAUDE.md`.

## Quyết định đã chốt

- Dịch Anh -> Việt, **chỉ dịch phụ đề của game**, bằng OCR chữ trên hình (không dịch giọng nói). Ví dụ mẫu: "Buy a new outfit" -> "Mua bộ đồ mới" hiện phía trên dòng gốc.
- Nguồn hình: **PS5 -> PC** (capture card hoặc PS Remote Play). Game không chạy trên PC nên không có tiến trình game, không có fullscreen/BattlEye để lo; app chỉ chụp cửa sổ hoặc vùng màn hình đang hiện hình PS5.
- Chạy 100% local, không gửi dữ liệu ra ngoài.
- Rust, một file `.exe` gọn nhẹ.
- Chỉ chụp hình bằng API của Windows. Không đọc bộ nhớ, không inject, không đụng file nào của game/console.
- Phải chạy được trên máy không rõ cấu hình: ưu tiên CPU, GPU chỉ là tăng tốc tùy chọn.
- Hiện bản dịch lên stream: ưu tiên mở một trang overlay nền trong suốt chỉ trên `127.0.0.1` để OBS thêm bằng Browser Source (giữ nền trong suốt tốt); phương án phụ là OBS Window Capture cửa sổ overlay.

## Kiến trúc (pipeline)

```
Chụp vùng phụ đề của cửa sổ hiện hình PS5 (Windows Graphics Capture)
  -> Phát hiện thay đổi (bỏ qua khung hình không đổi)
  -> OCR tiếng Anh (Windows.Media.Ocr có sẵn, chạy CPU, không cần tải model)
  -> Cache bản dịch (chuỗi lặp lại như menu: trả ngay, ~0 ms)
  -> Dịch Anh->Việt (model local nhỏ, CPU)
  -> Overlay trong suốt, hiện bản dịch ngay trên chữ gốc
  -> (stream) trang overlay localhost cho OBS Browser Source
```

Vì sao OCR thay vì nghe giọng: GTA đã có sẵn phụ đề tiếng Anh trên màn hình, đọc chữ chính xác hơn nhận dạng giọng (tên riêng, tiếng lóng), nhẹ hơn nhiều và không cần GPU. Game chạy trên PS5 nên GPU của PC gần như rảnh.

Ràng buộc:
- Phụ đề phải bật trong game (PS5) và đủ lớn/rõ trong hình capture để OCR đọc được (nên capture 1080p trở lên).
- Cửa sổ hiện hình PS5 trên PC (OBS Projector, Elgato 4K Capture Utility, PS Remote Play...) phải nhìn thấy được, không bị che/thu nhỏ khi app chụp. Cần xác nhận streamer dùng cách nào.
- Độ trễ capture card/Remote Play đã có sẵn trước khi app nhận hình; app chỉ cộng thêm phần của nó.
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
- [x] **1. Capture**: chụp cửa sổ/vùng hiện hình PS5 bằng Windows Graphics Capture, đo độ trễ, đo tải CPU
- [x] **2. OCR**: đọc chữ tiếng Anh từ ảnh mẫu GTA, đo tốc độ + độ chính xác
  - Dùng Windows.Media.Ocr (có sẵn, chạy CPU, không tải model). Thêm feature windows crate cần thiết như Media_Ocr, Graphics_Imaging, Storage_Streams.
  - Module mới `src/ocr.rs`: nhận `Frame`, trả về text và vị trí dòng. Báo lỗi rõ nếu máy thiếu gói ngôn ngữ OCR tiếng Anh.
  - Chỉ OCR khi vùng phụ đề đổi (dùng cờ `changed` của capture).
  - CLI mới: `translator-ai ocr --image file.bmp` và `ocr --window ... --region ...`.
  - Test: unit test cộng ảnh mẫu có chữ tiếng Anh (tự render hoặc xin ảnh phụ đề GTA từ người dùng). Thử nhiều cỡ chữ và nền. Nếu chữ nhỏ thì phóng to hoặc tăng tương phản trước khi OCR.
  - Mục tiêu tốc độ OCR 30-100 ms cho vùng phụ đề. Ghi số đo thật vào nhật ký.
- [ ] **3. Dịch**: benchmark vài model Anh->Việt chạy CPU, chọn cái cân bằng tốc độ/chất lượng; thêm cache + từ điển
- [ ] **4. Overlay**: cửa sổ trong suốt click-through, đặt bản dịch trên chữ gốc, chỉnh font/vị trí/vùng quét
- [ ] **5. Ghép pipeline**: đa luồng, bỏ qua khung không đổi, chống nhấp nháy khi chữ đổi
- [ ] **6. OBS**: trang overlay localhost cho Browser Source (nền trong suốt), kiểm tra trên OBS, hướng dẫn cài cho streamer
- [ ] **7. Tối ưu**: đo tải CPU/độ trễ khi chạy cùng OBS đang encode stream
- [ ] **8. Phát hành**: cài đặt (hotkey, font, vùng), build GitHub Actions, checksum SHA256, code signing, `cargo audit`

## Câu hỏi mở

1. (Tạm chốt, cần xác nhận với sangtran) Hình PS5 vào PC chỉ qua OBS. App chụp cửa sổ **OBS Projector** (chuột phải source > Windowed Projector). Nếu streamer dùng app khác (Elgato, Remote Play) thì chụp cửa sổ đó, cùng cách.
2. Cấu hình máy streamer (CPU/RAM), có thể để sau.
3. Có ảnh chụp màn hình GTA mẫu (có phụ đề) để test OCR không? Chưa có GTA 6 thì dùng GTA 5/ảnh mẫu.

## Nhật ký

- 2026-11-20: tạo khung dự án, chốt Rust + local + an toàn.
- 2026-11-20: đổi hướng từ nghe giọng nói sang OCR chữ trên màn hình theo ví dụ của streamer.
- 2026-11-20: chốt chỉ dịch phụ đề; nguồn hình là PS5 nối PC nên bỏ ràng buộc Borderless/anti-cheat, thêm overlay Browser Source cho OBS.
- 2026-11-20: **xong giai đoạn 1 (capture)**. Cài Rust 1.100 + VS Build Tools. Code: `src/capture.rs` (Windows Graphics Capture), `src/frame.rs` (vùng, hash đổi khung, BMP), `src/main.rs` (CLI `translator-ai capture --list|--window|--monitor|--region|--seconds|--save`). Dùng GPU, tự lùi về WARP (CPU) nếu không có GPU.
  - Test: 5 unit test pass, clippy sạch, chạy thật trên màn hình/cửa sổ: nhận ~48-50 khung/giây.
  - Đo: copy GPU->CPU vùng phụ đề 1200x150 tb **0.32 ms** (max 1.27); cả màn hình tb 4.9 ms. Nên chỉ copy vùng phụ đề. Phát hiện đổi khung đúng: vùng tĩnh chỉ 1 khung "đổi", vùng chuyển động gần như mọi khung.
  - Độ trễ khung -> app: tb 0.3-1.7 ms, max 32-40 ms (thỉnh thoảng trễ, đo bằng mốc thời gian của Windows nên sai số vài ms, có lúc ra số âm). File .exe release 215 KB.
  - Chưa đo tải CPU % và chưa thử với hình PS5/Elgato thật.
- 2026-11-20: **xong giai doan 2 (OCR)**. Code: `src/ocr.rs` (Windows.Media.Ocr, engine en-US, tra ve dong chu + khung bao), `frame.rs` them `decode_bmp` va `scale_up` (phong to bilinear), CLI `translator-ai ocr --image f.bmp | --window/--monitor --region ... --scale K` (chi OCR khi khung doi). Bao loi ro neu may thieu goi OCR tieng Anh.
  - Test: 12 unit test pass (5 test OCR tren anh tu render bang GDI: trang tren toi, toi tren sang, vang tren xanh, chu nho 14px phai phong to x3 moi doc duoc, anh trong khong ra dong nao), clippy sach, build release chay duoc (exe 248 KB).
  - Do: OCR vung 1200x150 chu 36px tb **3.4 ms** (release), CLI tren anh BMP 3.5-7 ms. Thap hon nhieu so voi muc tieu 30-100 ms, nhung anh mau la chu GDI sach, chu game that co vien/nen phuc tap se cham hon.
  - Chua kiem chung: anh phu de GTA that (chua co mau), duong `ocr --window` chup that (chua chay de khong in noi dung man hinh nguoi dung), do chinh xac tren font GTA. Region cat qua hep lam mat chu cuoi (da thay khi cat 400px).

