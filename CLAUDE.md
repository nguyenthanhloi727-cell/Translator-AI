# Translator-AI — Quy tắc dự án

App phụ đề realtime Anh -> Việt cho streamer (sangtran) chơi GTA 6 trên PS5 nối vào PC. App đọc phụ đề trong hình (OCR) rồi dịch. Chạy local dạng `.exe`, độ trễ thấp, nhẹ.
Tiến độ và kiến trúc: xem `progress.md`.

## Quy trình bắt buộc cho MỖI tính năng

1. Làm xong tính năng.
2. **Test** (unit test + chạy thử thật, đo độ trễ nếu là khâu trong pipeline). Test fail thì sửa, không push.
3. Test pass -> `git commit` rõ ràng -> `git push`.
4. Tick tính năng trong `progress.md` và ghi nhật ký (kết quả test, số đo độ trễ).

Không gộp nhiều tính năng vào một commit. Không push khi chưa test.

## An toàn (làm cho streamer, tuyệt đối không dính virus)

- Chạy 100% local. Không gửi hình/dữ liệu ra ngoài. Không telemetry. Không tự cập nhật ngầm.
- Chỉ chụp hình bằng API chuẩn của Windows (Windows Graphics Capture). **Cấm** inject DLL, hook vào tiến trình khác, đọc bộ nhớ, đụng file game/console (tránh bị antivirus báo).
- Overlay là cửa sổ riêng của app (trong suốt, click-through). Server cho OBS chỉ nghe trên `127.0.0.1`, không mở ra mạng.
- Không dùng packer/obfuscator (UPX, v.v.), không tải và chạy code từ mạng lúc runtime.
- Dependency: ít, từ nguồn uy tín, ghim version, chạy `cargo audit` trước khi release.
- Model tải một lần, kiểm tra SHA256, để trong thư mục riêng, không commit vào git.
- Bản release build bằng GitHub Actions từ source công khai, kèm checksum SHA256. Ký code (code signing) nếu có thể.
- Không commit secret, key, file model, ảnh chụp màn hình thu được.

## Công nghệ

- Rust (một file .exe gọn, không cần runtime như Python/.NET, ít bị antivirus báo hơn bản đóng gói PyInstaller).
- OCR: Windows.Media.Ocr có sẵn (CPU, không cần tải model).
- Dịch: model local nhẹ chạy CPU; chọn sau khi benchmark chất lượng tiếng Việt.
- Giữ code nhỏ gọn, tránh dependency thừa.

## Môi trường và quy ước làm việc

- Windows 11, PowerShell. Rust 1.100 và VS Build Tools đã cài. Nếu không thấy cargo thì chạy: `$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path`
- Crate `windows` ghim `=0.62.2`. Thêm feature khi cần, không nâng version tùy tiện.
- Trước khi commit: `cargo test`, `cargo clippy` sạch, `cargo build --release` chạy được.
- Chỉ `git add` đúng file cần. Không commit ảnh chụp màn hình, model, file .bmp thử.
- Câu commit kết thúc bằng dòng: `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`
- Người dùng nói tiếng Việt: trả lời ngắn gọn, tiếng Việt, nói rõ cái gì đã test và cái gì chưa kiểm chứng.
- Test chụp thật chỉ in số liệu, không lưu ảnh màn hình của người dùng.

## Lỗi hook trên máy người dùng

Khi ghi file có ký tự dấu lớn hơn (kể cả trong mũi tên của comment hay markdown), một hook trên máy tạo ra file rỗng 0 byte tên lạ trong thư mục dự án. Tránh viết dấu mũi tên và dấu lớn hơn trong nội dung mới. Trước khi commit, xóa file rỗng lạ (Length = 0) nếu có.

## Trạng thái code (cập nhật khi xong mỗi giai đoạn)

- `src/capture.rs`: Windows Graphics Capture, chụp cửa sổ hoặc màn hình chính, cắt vùng, tự lùi về WARP nếu không có GPU.
- `src/frame.rs`: `Region`, `Frame` (BGRA8 đóng gói chặt), `fnv1a`, `encode_bmp`, có unit test.
- `src/main.rs`: CLI `translator-ai capture` với `--list`, `--window`, `--monitor`, `--region x,y,w,h`, `--seconds`, `--save`.
- `src/ocr.rs`: OCR tiếng Anh bằng Windows.Media.Ocr, tự phóng x2 vùng thấp hơn 160 px, trả dòng + khung bao.
- `src/frame.rs` có thêm `decode_bmp`; `main.rs` có thêm CLI `translator-ai ocr --image|--window|--monitor|--region|--seconds`.
