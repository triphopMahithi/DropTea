import logging
import sys
import os
import json
from logging.handlers import RotatingFileHandler

class JsonFormatter(logging.Formatter):
    def format(self, record):
        log_record = {
            "timestamp": self.formatTime(record, self.datefmt),
            "level": record.levelname,
            "logger": record.name,
            "message": record.getMessage(),
        }
        # เพิ่ม module และ line เพื่อการ debug ที่ง่ายขึ้นใน Production
        if record.levelno >= logging.ERROR:
            log_record["module"] = record.module
            log_record["line"] = record.lineno
            
        return json.dumps(log_record)

# ✅ แก้ไขบรรทัดนี้: เพิ่ม max_size_mb และ backup_count เป็น arguments
def setup_logging(log_filename="logs/app.jsonl", debug_mode=False, max_size_mb=10, backup_count=5):
    log_folder = os.path.dirname(log_filename)
    if log_folder:
        os.makedirs(log_folder, exist_ok=True)

    logger = logging.getLogger()
    
    # Reset handlers เก่าป้องกัน Log เบิ้ลเวลา reload
    if logger.hasHandlers():
        logger.handlers.clear()

    # ระดับ Log หลัก
    root_level = logging.DEBUG if debug_mode else logging.INFO
    logger.setLevel(root_level)

    # ✅ คำนวณขนาดไฟล์จาก MB เป็น Bytes
    max_bytes = max_size_mb * 1024 * 1024

    # 1. File Handler (JSON Format for tools like ELK/Splunk)
    file_handler = RotatingFileHandler(
        log_filename, 
        maxBytes=max_bytes,       # ใช้ค่าที่รับมา
        backupCount=backup_count, # ใช้ค่าที่รับมา
        encoding='utf-8'
    )
    file_handler.setFormatter(JsonFormatter(datefmt='%Y-%m-%d %H:%M:%S'))
    
    # 2. Console Handler (Human Readable)
    console_formatter = logging.Formatter('%(asctime)s [%(levelname)s] %(name)s: %(message)s', datefmt='%H:%M:%S')
    console_handler = logging.StreamHandler(sys.stdout)
    console_handler.setFormatter(console_formatter)
    console_handler.setLevel(logging.INFO) # Console ไม่ต้องรกมาก ให้ไปดูละเอียดในไฟล์เอา

    logger.addHandler(file_handler)
    logger.addHandler(console_handler)
    
    # ลดความพูดมากของ Library ภายนอก
    lib_level = logging.DEBUG if debug_mode else logging.WARNING
    for lib in ["droptea_core", "mdns_sd", "asyncio", "zeroconf"]:
        logging.getLogger(lib).setLevel(lib_level)

    if debug_mode:
        print(f"🔧 DEBUG MODE: ENABLED (Log: {log_filename}, Max: {max_size_mb}MB x {backup_count})")

    return logger