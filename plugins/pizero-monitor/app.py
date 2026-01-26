"""
==============================================================================
pizero_monitor.py - Pi Zero Ultra-Lightweight Monitor
==============================================================================

Minimal monitoring for Pi Zero (416MB RAM):
- CPU Temperature
- RAM Usage
- Uptime

NO LEDs, NO buzzer - just reports data back to Hub.
Designed to use minimal resources.

Build:
    componentize-py -d ../../wit -w pi-monitor-plugin componentize app -o pizero-monitor.wasm
"""

from wit_world.exports import PiMonitorLogic
from wit_world.exports.pi_monitor_logic import PiStats
from wit_world.imports import gpio_provider, system_info


class PiMonitorLogic(PiMonitorLogic):
    def poll(self) -> PiStats:
        cpu_temp = gpio_provider.get_cpu_temp()
        cpu_usage = system_info.get_cpu_usage()
        used_mb, total_mb = system_info.get_memory_usage()
        uptime = system_info.get_uptime()
        
        # Minimal logging, no LED/buzzer control
        print(f"📊 [PIZERO] {cpu_temp:.1f}°C | RAM: {used_mb}/{total_mb}MB")
        
        return PiStats(
            cpu_temp=cpu_temp,
            cpu_usage=cpu_usage,
            memory_used_mb=used_mb,
            memory_total_mb=total_mb,
            uptime_seconds=uptime,
            timestamp_ms=gpio_provider.get_timestamp_ms()
        )
