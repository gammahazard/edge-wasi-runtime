"""
==============================================================================
pi4_monitor.py - Pi 4 Spoke System Monitor
==============================================================================

Monitors Pi 4 system health:
- CPU Temperature (75°C threshold for alert)
- RAM Usage
- Uptime

Controls LED 3 for system status.

Build:
    componentize-py -d ../../wit -w pi-monitor-plugin componentize app -o pi4-monitor.wasm
"""

from wit_world.exports import PiMonitorLogic
from wit_world.exports.pi_monitor_logic import PiStats
from wit_world.imports import gpio_provider, led_controller, system_info, buzzer_controller


class PiMonitorLogic(PiMonitorLogic):
    def poll(self) -> PiStats:
        cpu_temp = gpio_provider.get_cpu_temp()
        cpu_usage = system_info.get_cpu_usage()
        used_mb, total_mb = system_info.get_memory_usage()
        uptime = system_info.get_uptime()
        
        # Pi 4: Use LED 3 for health status
        if cpu_temp > 80.0:
            led_controller.set_led(3, 255, 0, 0)  # Red - Critical
            buzzer_controller.beep(2, 100, 100)
            print(f"🔴 [PI4] CRITICAL: {cpu_temp:.1f}°C")
        elif cpu_temp > 75.0:
            led_controller.set_led(3, 255, 100, 0)  # Orange - Warning
            buzzer_controller.buzz(30)
            print(f"🟠 [PI4] HOT: {cpu_temp:.1f}°C")
        elif cpu_temp > 60.0:
            led_controller.set_led(3, 255, 200, 0)  # Yellow - Warm
            print(f"🟡 [PI4] Warm: {cpu_temp:.1f}°C")
        else:
            led_controller.set_led(3, 0, 255, 0)  # Green - OK
            print(f"🟢 [PI4] OK: {cpu_temp:.1f}°C | RAM: {used_mb}/{total_mb}MB")
        
        led_controller.sync_leds()
        
        return PiStats(
            cpu_temp=cpu_temp,
            cpu_usage=cpu_usage,
            memory_used_mb=used_mb,
            memory_total_mb=total_mb,
            uptime_seconds=uptime,
            timestamp_ms=gpio_provider.get_timestamp_ms()
        )
