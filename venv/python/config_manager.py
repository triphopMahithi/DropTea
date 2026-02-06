# config_manager.py
import os
import tomllib
import re
import logging
from dataclasses import dataclass

logger = logging.getLogger("ConfigManager")

@dataclass
class ServerConfig:
    host: str
    port: int
    buffer_size: int
    timeout: int
    mode: str

@dataclass
class StorageConfig:
    save_path: str
    temp_path: str

@dataclass
class ProtocolConfig:
    header_format: str
    header_size: int

@dataclass
class DevConfig:
    enabled: bool

@dataclass
class LoggingConfig:
    debug: bool
    file_path: str
    max_size_mb: int = 10
    backup_count: int = 5

class Settings:
    _instance = None
    
    def __new__(cls, config_path="config.toml"):
        if cls._instance is None:
            cls._instance = super(Settings, cls).__new__(cls)
            cls._instance.path = config_path
            cls._instance.load()
        return cls._instance

    def load(self):
        if not os.path.exists(self.path):
            raise FileNotFoundError(f"Configuration file not found: {self.path}")

        try:
            with open(self.path, "rb") as f:
                data = tomllib.load(f)
                
                self.server = ServerConfig(**data.get('server', {}))
                self.storage = StorageConfig(**data.get('storage', {}))
                self.protocol = ProtocolConfig(**data.get('protocol', {'header_format': '128sQ32s', 'header_size': 168}))
                self.dev = DevConfig(**data.get('dev', {'enabled': False}))
                
                log_data = data.get('logging', {})
                self.logging = LoggingConfig(
                    debug=log_data.get('debug', False),
                    file_path=log_data.get('file_path', 'logs/app.jsonl'),
                    max_size_mb=log_data.get('max_size_mb', 10),
                    backup_count=log_data.get('backup_count', 5)
                )

                # Ensure critical directories exist
                os.makedirs(self.storage.save_path, exist_ok=True)
                os.makedirs(self.storage.temp_path, exist_ok=True)
                
        except tomllib.TOMLDecodeError as e:
            logger.error(f"Invalid TOML format in {self.path}: {e}")
            raise ValueError(f"Config syntax error: {e}")
        except OSError as e:
            logger.error(f"Could not access config or create directories: {e}")
            raise

class ConfigEditor:
    """Helper class to safely update TOML files while preserving comments."""
    def __init__(self, path):
        self.path = path

    def update_key(self, section: str, key: str, value) -> tuple[bool, str]:
        if not os.path.exists(self.path):
            return False, "Configuration file not found"
        
        # Format value for TOML
        if isinstance(value, str):
            val_str = f'"{value}"'
        elif isinstance(value, bool):
            val_str = "true" if value else "false"
        else:
            val_str = str(value)
        
        try:
            with open(self.path, 'r', encoding='utf-8') as f:
                lines = f.readlines()
        except UnicodeDecodeError:
            return False, "Encoding error: File is not UTF-8 compatible"
        except PermissionError:
            return False, "Permission denied: Cannot read configuration file"

        new_lines = []
        in_section = False
        updated = False
        
        # Regex to match 'key = value'
        key_pattern = re.compile(rf'^\s*{key}\s*=\s*(.*)')

        for line in lines:
            stripped = line.strip()
            
            # Identify section
            if stripped.startswith('[') and stripped.endswith(']'):
                current_sec = stripped[1:-1]
                in_section = (current_sec == section)
            
            # Update key if found within correct section
            if in_section and key_pattern.match(stripped):
                # Preserve existing comments
                comment = ""
                if "#" in line:
                    parts = line.split("#", 1)
                    if len(parts) > 1:
                        comment = " #" + parts[1].rstrip()
                
                # Preserve existing indentation
                indent = line[:line.find(key)]
                new_lines.append(f"{indent}{key} = {val_str}{comment}\n")
                updated = True
            else:
                new_lines.append(line)

        if updated:
            try:
                with open(self.path, 'w', encoding='utf-8') as f:
                    f.writelines(new_lines)
                return True, f"Updated [{section}] {key} = {val_str}"
            except PermissionError:
                return False, "Permission denied: Cannot write to configuration file"
            except Exception as e:
                logger.error(f"Failed to write config: {e}")
                return False, f"Write error: {e}"
        
        return False, f"Key '{key}' not found in section [{section}]"