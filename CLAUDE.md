# Translator-AI — Quy tắc dự án

App phụ đề realtime Anh -> Việt cho streamer (sangtran) chơi GTA 6 trên PC. Chạy local dạng `.exe`, độ trễ thấp, nhẹ.
Tiến độ và kiến trúc: xem `progress.md`.

## Quy trình bắt buộc cho MỖI tính năng

1. Làm xong tính năng.
2. **Test** (unit test + chạy thử thật, đo độ trễ nếu là khâu trong pipeline). Test fail thì sửa, không push.
3. Test pass -> `git commit` rõ ràng -> `git push`.
4. Tick tính năng trong `progress.md` và ghi nhật ký (kết quả test, số đo độ trễ).

Không gộp nhiều tính năng vào một commit. Không push khi chưa test.

## An toàn (làm cho streamer, tuyệt đối không dính virus)

- Chạy 100% local. Không gửi audio/dữ liệu ra ngoài. Không telemetry. Không tự cập nhật ngầm.
- Chỉ bắt audio qua WASAPI loopback / process loopback. **Cấm** inject DLL, hook vào game, đọc bộ nhớ game, đụng file game (tránh anti-cheat BattlEye và tránh bị antivirus báo).
- Overlay là cửa sổ riêng của app (trong suốt, click-through), không vẽ vào tiến trình game.
- Không dùng packer/obfuscator (UPX, v.v.), không tải và chạy code từ mạng lúc runtime.
- Dependency: ít, từ nguồn uy tín, ghim version, chạy `cargo audit` trước khi release.
- Model tải một lần, kiểm tra SHA256, để trong thư mục riêng, không commit vào git.
- Bản release build bằng GitHub Actions từ source công khai, kèm checksum SHA256. Ký code (code signing) nếu có thể.
- Không commit secret, key, file model, file audio thu được.

## Công nghệ

- Rust (một file .exe gọn, không cần runtime như Python/.NET, ít bị antivirus báo hơn bản đóng gói PyInstaller).
- ASR: whisper.cpp (qua `whisper-rs`) hoặc tương đương, chạy GPU.
- Dịch: model local nhẹ; chọn sau khi benchmark chất lượng tiếng Việt.
- Giữ code nhỏ gọn, tránh dependency thừa.
