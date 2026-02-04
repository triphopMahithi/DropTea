import asyncio
import os
import logging
from dataclasses import dataclass, field
from typing import Dict, Optional
from droptea_core import DropTeaEngine
from events import TransferEvents

logger = logging.getLogger("TransferManager")

@dataclass(order=True)
class TransferTask:
    priority: int
    file_path: str = field(compare=False)
    peer_ip: str = field(compare=False); peer_port: int = field(compare=False)
    task_id: str = field(compare=False)
    # 🔥 1. เพิ่ม field นี้เพื่อระบุ OS ปลายทาง
    target_os: Optional[str] = field(default=None, compare=False)

class AsyncTransferManager:
    def __init__(self, discovery_service, event_handler: TransferEvents, device_name="Unknown", engine=None, loop=None, cert_verifier=None):
        self.queue = asyncio.PriorityQueue()
        self.discovery = discovery_service
        self.events = event_handler
        self.active_tasks = {}
        self.device_name = device_name
        self.loop = loop
        self.engine = engine if engine else DropTeaEngine()
        self.cert_verifier = cert_verifier 
        self._running = True

    def _rust_callback(self, *args):
        try:
            if not args: return
            event = args[0]

            if event == "ask_verify_certificate":
                if self.cert_verifier:
                    return self.cert_verifier(*args)
                return False 

            if len(args) < 3: return
            task_id, data = args[1], args[2]

            if event == "START":
                self.events.on_start(task_id, str(data))
            
            elif event == "PROGRESS":
                try:
                    if isinstance(data, str) and "|" in data:
                        c, t = map(int, data.split("|"))
                    elif isinstance(data, (list, tuple)):
                        c, t = data
                    else: return
                    self.events.on_progress(task_id, c, t)
                except: pass
            
            elif event == "COMPLETED":
                self.events.on_status_change(task_id, "COMPLETED")
            
            elif event == "ERROR":
                self.events.on_error(task_id, str(data))

        except Exception as e:
            logger.error(f"Callback error: {e}")

    async def add_task(self, file_path, peer_name):
        if not self.discovery: 
            self.events.on_error("system", "Discovery service not ready")
            return
        
        peer_info = self.discovery.peers.get(peer_name)
        if not peer_info: 
            self.events.on_error("system", f"Peer {peer_name} not found")
            return

        task_id = os.path.basename(file_path)
        
        # 🔥 2. เพิ่ม Logic ตรวจสอบ OS จากชื่อเครื่อง (Heuristic)
        detected_os = None
        if isinstance(peer_info, dict): 
            ip, port = peer_info['ip'], peer_info['port']
            p_name = peer_info.get('name', '').lower()
            
            # ถ้าชื่อเครื่องมีคำว่า iphone หรือ ipad ให้ถือว่าเป็น iOS
            if "iphone" in p_name or "ipad" in p_name:
                detected_os = "ios"
            elif "mac" in p_name:
                detected_os = "macos"
        else: 
            ip, port = peer_info

        # สร้าง Task โดยระบุ target_os ไปด้วย
        task = TransferTask(10, file_path, ip, port, task_id, target_os=detected_os)
        await self.queue.put(task)
        self.active_tasks[task_id] = task
        
        if hasattr(self.events, 'on_task_queued'):
             self.events.on_task_queued(task_id, task_id, "Waiting...")
        else:
             self.events.on_task_added(task_id, task_id, side="SEND")

    async def start_worker(self):
        while self._running:
            task = await self.queue.get()
            
            # 🔥 3. ส่ง task.target_os ไปให้ Rust
            # (ถ้าเป็น "ios" Rust จะรู้ทันทีว่าต้องส่งแบบ Raw)
            self.engine.send_file(
                task.peer_ip, 
                task.peer_port, 
                task.file_path, 
                task.task_id, 
                self._rust_callback,
                self.device_name,
                task.target_os 
            )
            self.queue.task_done()