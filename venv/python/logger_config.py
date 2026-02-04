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
            "module": record.module,
            "line": record.lineno,
        }
        if hasattr(record, 'task_id'):
            log_record['task_id'] = record.task_id
        return json.dumps(log_record)

def setup_logging(log_filename="logs/app.jsonl", debug_mode=False):
    log_folder = os.path.dirname(log_filename)
    if log_folder:
        os.makedirs(log_folder, exist_ok=True)

    logger = logging.getLogger()
    
    root_level = logging.DEBUG if debug_mode else logging.INFO
    logger.setLevel(root_level)
    
    if logger.hasHandlers(): return logger

    file_formatter = JsonFormatter(datefmt='%Y-%m-%d %H:%M:%S')
    file_handler = RotatingFileHandler(log_filename, maxBytes=10*1024*1024, backupCount=5, encoding='utf-8')
    file_handler.setFormatter(file_formatter)
    
    console_formatter = logging.Formatter('%(asctime)s [%(levelname)s] %(name)s: %(message)s', datefmt='%H:%M:%S')
    console_handler = logging.StreamHandler(sys.stdout)
    console_handler.setFormatter(console_formatter)
    console_handler.setLevel(logging.INFO) 

    logger.addHandler(file_handler)
    logger.addHandler(console_handler)
    
    rust_log_level = logging.DEBUG if debug_mode else logging.WARNING
    
    logging.getLogger("droptea_core").setLevel(rust_log_level)
    logging.getLogger("mdns_sd").setLevel(rust_log_level)
    logging.getLogger("dns_parser").setLevel(rust_log_level)
    logging.getLogger("asyncio").setLevel(rust_log_level)
    logging.getLogger("zeroconf").setLevel(rust_log_level)

    if debug_mode:
        print(f"🔧 DEBUG MODE: ENABLED (Verbose logs -> {log_filename})")

    return logger