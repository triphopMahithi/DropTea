import os
from config_manager import ConfigEditor, Settings

CONFIG_CONTENT = """
title = "Test App"
[server]
port = 8000 # Old Port
"""

def test_config_update(tmp_path):
    # จำลองไฟล์ config ขึ้นมาใน Temp Dir
    f = tmp_path / "test_config.toml"
    f.write_text(CONFIG_CONTENT, encoding="utf-8")
    
    # ทดสอบการแก้ค่า
    editor = ConfigEditor(str(f))
    success, msg = editor.update_key("server", "port", 9090)
    
    assert success is True
    
    # อ่านกลับมาเช็ค
    new_content = f.read_text(encoding="utf-8")
    assert "port = 9090" in new_content
    assert "# Old Port" in new_content # Comment ต้องอยู่ครบ