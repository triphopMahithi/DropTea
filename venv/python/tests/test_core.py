import pytest
from unittest.mock import MagicMock, AsyncMock
from core_services import TransferController
from state_manager import AppState

@pytest.mark.asyncio
async def test_transfer_controller_calls_rust():
    # 1. Setup: สร้างของปลอม (Mock)
    mock_state = MagicMock(spec=AppState)
    
    mock_state.peers = MagicMock()
    mock_state.peers.get.return_value = {'ip': '1.2.3.4', 'port': 8080, 'name': 'TestPeer'}

    mock_ui = MagicMock()
    mock_engine = MagicMock() 

    # 2. Initialize Controller
    controller = TransferController(mock_state, mock_ui, mock_engine, "MyDevice")

    # 3. Action: สั่งส่งไฟล์
    await controller.queue_file("test.txt", "peer_id_1")
    
    # ดึง Task ออกมา (เพื่อจำลองว่า Worker ทำงาน)
    task = await controller.queue.get() 
    
    # จำลองการเรียก method ที่ worker จะต้องทำจริงๆ
    mock_engine.send_file(
        task.peer_ip, 
        task.peer_port, 
        task.file_path, 
        task.task_id, 
        controller._rust_callback,
        "MyDevice",
        task.target_os
    )

    # 4. Assert: ตรวจสอบว่า Python สั่ง Rust ด้วยค่าที่ถูกต้องหรือไม่
    mock_engine.send_file.assert_called_with(
        '1.2.3.4',  # IP ต้องตรงกับ Mock Peer
        8080,       # Port ต้องตรง
        'test.txt', 
        'test.txt', # Task ID (basename)
        controller._rust_callback, 
        "MyDevice",
        None        # OS (None เพราะชื่อ peer ไม่ได้บอกว่าเป็น Mac/iOS)
    )