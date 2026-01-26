"""
==============================================================================
revpi_monitor.py - Specialized monitoring for RevPi Hub
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
        
        # RevPi Specific: Higher thermal headroom (85C), use LED 0 for Hub Status
        if cpu_temp > 85.0:
            led_controller.set_led(0, 255, 0, 100) # Purple Alert
        
        # Note: Hub doesn't beep as often as spokes to avoid annoying the operator
        
        return PiStats(
            cpu_temp=cpu_temp,
            cpu_usage=cpu_usage,
            memory_used_mb=used_mb,
            memory_total_mb=total_mb,
            uptime_seconds=uptime,
            timestamp_ms=gpio_provider.get_timestamp_ms()
        )
