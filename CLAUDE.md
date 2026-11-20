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
