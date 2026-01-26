"""
==============================================================================
pi4_monitor.py - Specialized monitoring for Raspberry Pi 4 Spokes
==============================================================================
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
        
        # Pi 4 Specific: Use LED 3 for health, Flash Red if > 75C
        if cpu_temp > 75.0:
            led_controller.set_led(3, 255, 0, 0)
            buzzer_controller.beep(2, 50, 50)
        else:
            led_controller.set_led(3, 0, 255, 0)
            
        led_controller.sync_leds()
        
        return PiStats(
            cpu_temp=cpu_temp,
            cpu_usage=cpu_usage,
            memory_used_mb=used_mb,
            memory_total_mb=total_mb,
            uptime_seconds=uptime,
            timestamp_ms=gpio_provider.get_timestamp_ms()
        )
