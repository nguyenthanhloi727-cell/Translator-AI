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
- [ ] **2. OCR**: đọc chữ tiếng Anh từ ảnh mẫu GTA, đo tốc độ + độ chính xác
  - Dùng Windows.Media.Ocr (có sẵn, chạy CPU, không tải model). Thêm feature windows crate cần thiết như Media_Ocr, Graphics_Imaging, Storage_Streams.
  - Module mới `src/ocr.rs`: nhận `Frame`, trả về text và vị trí dòng. Báo lỗi rõ nếu máy thiếu gói ngôn ngữ OCR tiếng Anh.
  - Chỉ OCR khi vùng phụ đề đổi (dùng cờ `changed` của capture).
  - CLI mới: `translator-ai ocr --image file.bmp` và `ocr --window ... --region ...`.
  - Test: unit test cộng ảnh mẫu có chữ tiếng Anh (tự render hoặc xin ảnh phụ đề GTA từ người dùng). Thử nhiều cỡ chữ và nền. Nếu chữ nhỏ thì phóng to hoặc tăng tương phản trước khi OCR.
  - Mục tiêu tốc độ OCR 30-100 ms cho vùng phụ đề. Ghi số đo thật vào nhật ký.
- [x] **3. Dịch**: benchmark vài model Anh->Việt chạy CPU, chọn cái cân bằng tốc độ/chất lượng; thêm cache + từ điển (xong, nhưng tốc độ cache miss ~520 ms, CHƯA đạt mục tiêu 50-150 ms, xem nhật ký)
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
- 2026-11-20: **xong giai đoạn 3 (dịch), chất lượng đạt, tốc độ chưa đạt**. Code: `src/translate.rs` (NLLB-200-distilled-600M qua CTranslate2 int8 bằng crate `ct2rs`, cache 512 câu, từ điển tên riêng `glossary.txt`), CLI `translator-ai translate [--bench] [--threads N] ["câu"]` và `translator-ai model verify`. Model để ở `models/` (ngoài git), SHA256 pin trong `model-manifest.txt`, kiểm tra mỗi lần nạp (~0.4 s). Tạo model: `scripts/prepare-model.cmd`. Build: `scripts/build.cmd` (cần cmake + ninja + MSVC, link CRT tĩnh).
  - Benchmark Python CTranslate2 int8 (MKL), 10 câu kiểu GTA, i5-14600KF: opus-mt-en-vi tb 23-31 ms nhưng dịch sai nặng câu ngắn ("Buy a new outfit" ra rác); m2m100-418M tb 175-210 ms, nhiều câu gượng; **NLLB-600M tb 228-265 ms, dịch tự nhiên nhất** (giữ đúng Vice City, Leonida Keys). envit5 không convert được. Chọn NLLB-600M.
  - Đo thật bản Rust (ruy, greedy, 6 luồng): **cache miss tb 520-540 ms** (365-740 ms), cache hit 0.0005 ms, nạp model 1.3 s. Chậm hơn Python gấp ~2 vì backend `ruy` kém MKL/oneDNN. Thử `mkl` (build script tải binary từ mạng, lỗi) và `dnnl` (lỗi link do trộn CRT) đều chưa chạy được, bỏ.
  - Test: 11 unit test pass (gồm 1 test chạy model thật), clippy sạch, release build ok (.exe 5.2 MB, chưa tính model 622 MB). Từ điển 16 mục, tên riêng giữ đúng trong test.
  - Việc còn lại để đạt 50-150 ms: sửa link oneDNN (cung cấp symbol math, hoặc build cả chuỗi bằng /MD), hoặc dùng model nhỏ hơn/chưng cất riêng cho phụ đề GTA, hoặc dịch song song với OCR dòng kế. Chưa đo trên máy yếu của streamer. Số liệu Python ở trên là máy này, MKL.
- 2026-11-20: **thử tăng tốc dịch, chưa đạt 150 ms, không đổi code**.
  - oneDNN (`dnnl`): build 4 lần đều lỗi link vì trộn CRT tĩnh/động (`esaxx-rs` luôn /MT, oneDNN và glue của CTranslate2 /MD, thiếu `scalbnf`/`fminf`/`expm1f`; shim 3 hàm không đủ). Bỏ.
  - Giới hạn từ vựng đầu ra (`vmap.txt`, bật `use_vmap`): lớp xuất NLLB có 256k token. Python MKL, 6 luồng: 26k token tb 159 ms (từ 230-300), 44k token 172 ms, 66k token 197 ms, nhưng càng ít token càng sai tên riêng ("Vice City" ra "thành phố", "Ocean Drive" ra "đường xe Đại Dương"), và 66k vẫn dịch "Lucia" thành "Lucy". Trên bản Rust (66k token) nhanh hơn ~35-40%.
  - Số đo Rust lần này nhiễu: máy đang chạy game (CPU ~50%), baseline lên 900-1000 ms (lúc máy rảnh là 520-540 ms), vmap 66k còn ~590 ms cùng điều kiện. Chưa có số sạch.
  - Kết luận: vmap đánh đổi chất lượng lấy ~35% tốc độ, chưa đủ chạm 150 ms nên không bật. Hướng còn lại: MKL (cần nguồn MKL đáng tin và build không tải binary lúc build), hoặc model nhỏ hơn/chưng cất riêng cho câu phụ đề, hoặc chấp nhận ~500 ms vì dịch chạy luồng riêng, cache chặn câu lặp và phụ đề GTA thường hiện vài giây.

- 2026-11-20: người dùng chấp nhận độ trễ dịch ~0,5 giây (cache miss), bỏ mục tiêu 50-150 ms. Giai đoạn 3 coi là xong. Đo lại ở giai đoạn 7 trên máy streamer.
