"""
==============================================================================
revpi_monitor.py - RevPi Hub System Monitor
==============================================================================

Monitors RevPi Connect 4 (Hub) system health:
- CPU Temperature (industrial rated, higher threshold)
- RAM Usage
- Uptime

The Hub has higher thermal tolerance (85°C) and doesn't beep as much.

Build:
    componentize-py -d ../../wit -w pi-monitor-plugin componentize app -o revpi-monitor.wasm
"""

from wit_world.exports import PiMonitorLogic
from wit_world.exports.pi_monitor_logic import PiStats
from wit_world.imports import gpio_provider, led_controller, system_info


class PiMonitorLogic(PiMonitorLogic):
    def poll(self) -> PiStats:
        cpu_temp = gpio_provider.get_cpu_temp()
        cpu_usage = system_info.get_cpu_usage()
        used_mb, total_mb = system_info.get_memory_usage()
        uptime = system_info.get_uptime()
        
        # RevPi: Higher thermal tolerance, use LED 0 for hub status
        # No buzzer here to avoid annoying operator
        if cpu_temp > 85.0:
            led_controller.set_led(0, 255, 0, 100)  # Purple - Alert
            print(f"🟣 [HUB] HOT: {cpu_temp:.1f}°C")
        elif cpu_temp > 70.0:
            led_controller.set_led(0, 255, 200, 0)  # Yellow - Warm
            print(f"🟡 [HUB] Warm: {cpu_temp:.1f}°C")
        else:
            led_controller.set_led(0, 0, 255, 0)  # Green - OK
            print(f"🟢 [HUB] OK: {cpu_temp:.1f}°C | RAM: {used_mb}/{total_mb}MB | Up: {uptime//3600}h")
        
        return PiStats(
            cpu_temp=cpu_temp,
            cpu_usage=cpu_usage,
            memory_used_mb=used_mb,
            memory_total_mb=total_mb,
            uptime_seconds=uptime,
            timestamp_ms=gpio_provider.get_timestamp_ms()
        )
