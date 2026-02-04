import socket
import asyncio
import shutil, sys
from functools import partial
from droptea_core import compress_folder, extract_zip 

def format_bytes(size):
    power = 2**10
    n = 0
    power_labels = {0 : '', 1: 'K', 2: 'M', 3: 'G', 4: 'T'}
    while size > power:
        size /= power
        n += 1
    return f"{size:.2f} {power_labels[n]}B"

def get_free_port(start_port=8080, max_tries=100):
    for port in range(start_port, start_port + max_tries):
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            try:
                s.bind(('0.0.0.0', port))
                return port
            except OSError:
                continue
    raise OSError(f"No free ports found in range {start_port}-{start_port + max_tries}")

def draw_ascii_bar(current, total, filename, speed_bps=0, elapsed=0):
    if total <= 0: return

    percent = current / total
    term_width = shutil.get_terminal_size().columns
    bar_width = max(10, term_width - 65) 
    
    filled_len = int(bar_width * percent)
    bar = "█" * filled_len + "░" * (bar_width - filled_len)
    
    percent_str = f"{int(percent * 100)}%"
    progress_str = f"{format_bytes(current)}/{format_bytes(total)}"
    speed_str = f"{format_bytes(speed_bps)}/s"
    time_str = f"{int(elapsed)}s"
    
    if len(filename) > 15:
        filename = filename[:12] + "..."

    output = (
        f"\r{filename} "
        f"[{bar}] {percent_str} "
        f"| {progress_str} "
        f"| {speed_str} "
        f"| {time_str}"
    )
    
    padding = " " * max(0, term_width - len(output) - 1)
    sys.stdout.write(output + padding)
    sys.stdout.flush()
    
async def async_compress_folder(folder_path, output_path):
    loop = asyncio.get_running_loop()
    return await loop.run_in_executor(None, partial(compress_folder, folder_path, output_path))

async def async_extract_zip(zip_path, extract_to):
    loop = asyncio.get_running_loop()
    return await loop.run_in_executor(None, partial(extract_zip, zip_path, extract_to))