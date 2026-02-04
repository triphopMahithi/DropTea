import logging
from droptea_core import DropTeaEngine

logger = logging.getLogger("Receiver")

class AsyncReceiver:
    def __init__(self, event_handler, engine=None, loop=None):
        self.events = event_handler
        self.loop = loop 
        # ✅ ไม่ต้องรับ save_path จาก config แล้ว Rust จัดการเอง
        self.engine = engine if engine else DropTeaEngine()

    def _rust_callback(self, event, task_id, data):
        def _safe_update():
            try:
                if event == "Incoming":
                    str_data = str(data)
                    
                    # ✅ Case 1: คำขอส่งไฟล์ (Request)
                    if str_data.startswith("[[REQUEST]]|"):
                        parts = str_data.replace("[[REQUEST]]|", "").split("|")
                        if len(parts) >= 4:
                            fname, fsize, sender, device = parts[0], parts[1], parts[2], parts[3]
                            self.events.handle_incoming_request(task_id, fname, fsize, sender, device)
                    
                    # ✅ Case 2: เริ่มต้นส่ง (Start)
                    elif str_data.startswith("[[START]]|"):
                        fname = str_data.replace("[[START]]|", "")
                        self.events.on_start(task_id, fname)
                    
                    # Ignore malformed data
                    elif " [from " in str_data: 
                        pass 
                    else:
                        logger.debug(f"Ignored malformed Incoming data: {str_data}")

                elif event == "START": 
                    self.events.on_start(task_id, data)
                
                elif event == "PROGRESS":
                    try:
                        if isinstance(data, str) and "|" in data: c, t = map(int, data.split("|"))
                        else: c, t = data
                        self.events.on_progress(task_id, c, t)
                    except: pass
                
                elif event == "COMPLETED": 
                    self.events.on_status_change(task_id, "COMPLETED")
                
                elif event == "ERROR": 
                    err_msg = str(data)
                    if "deadline" in err_msg or "time" in err_msg.lower():
                        err_msg = "Timeout / No Response"
                    self.events.on_error(task_id, err_msg)

                elif event == "REJECTED":
                    self.events.on_reject(task_id, str(data))

                elif event == "SERVER_STARTED": 
                    logger.info(data)
                elif event == "PEER_FOUND": 
                    logger.debug(f"Peer: {data}")

            except Exception as e: logger.error(f"Callback error: {e}")

        if self.loop: self.loop.call_soon_threadsafe(_safe_update)
        else: _safe_update()
    
    # ❌ ลบฟังก์ชัน start() ออก เพราะ main.py เป็นคนสั่ง start_server เองแล้ว