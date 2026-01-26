"""
Pi4 Monitor Plugin - System health monitoring for Raspberry Pi 4
Controls LED 3 for CPU temperature status
"""
from wit_world.exports import PiMonitorLogic
from wit_world.exports.pi_monitor_logic import PiStats
from wit_world.imports import gpio_provider, led_controller, system_info, buzzer_controller


class PiMonitorLogic(PiMonitorLogic):
    def poll(self) -> PiStats:
        cpu_temp = gpio_provider.get_cpu_temp()
        cpu_usage = system_info.get_cpu_usage()
        mem_used, mem_total = system_info.get_memory_usage()
        uptime = system_info.get_uptime()
        timestamp = gpio_provider.get_timestamp_ms()
        
        # LED 3 for CPU temp
        if cpu_temp > 75.0:
            led_controller.set_led(3, 255, 0, 0)  # Red - critical
            buzzer_controller.beep(2, 50, 50)
            print(f"🔴 [PI4] CRITICAL: {cpu_temp:.1f}°C")
        elif cpu_temp > 60.0:
            led_controller.set_led(3, 255, 100, 0)  # Orange - warm
            print(f"🟠 [PI4] Warm: {cpu_temp:.1f}°C")
        else:
            led_controller.set_led(3, 0, 255, 0)  # Green - OK
            print(f"🟢 [PI4] OK: {cpu_temp:.1f}°C")
        
        led_controller.sync_leds()
        
        return PiStats(
            cpu_temp=cpu_temp,
            cpu_usage=cpu_usage,
            memory_used_mb=mem_used,
            memory_total_mb=mem_total,
            uptime_seconds=uptime,
            timestamp_ms=timestamp
        )
