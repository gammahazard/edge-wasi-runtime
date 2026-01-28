"""
Pi4 Monitor Plugin - System health monitoring for Raspberry Pi 4
Controls LED 3 for CPU temperature status
Controls cooling fan via GPIO 27 relay with hysteresis
"""
from wit_world.exports import PiMonitorLogic
from wit_world.exports.pi_monitor_logic import PiStats
from wit_world.imports import gpio_provider, led_controller, system_info, buzzer_controller, fan_controller

# Fan temperature thresholds with hysteresis (2°C deadband)
FAN_ON_THRESHOLD = 40.0   # Turn fan ON when CPU temp exceeds this
FAN_OFF_THRESHOLD = 28.0  # Turn fan OFF when CPU temp drops below this


class PiMonitorLogic(PiMonitorLogic):
    def poll(self) -> PiStats:
        cpu_temp = gpio_provider.get_cpu_temp()
        cpu_usage = system_info.get_cpu_usage()
        mem_used, mem_total = system_info.get_memory_usage()
        uptime = system_info.get_uptime()
        timestamp = gpio_provider.get_timestamp_ms()
        
        # Get current fan state
        current_fan_state = fan_controller.get_fan_state()
        
        # Fan control with hysteresis to prevent rapid cycling
        if cpu_temp >= FAN_ON_THRESHOLD and not current_fan_state:
            fan_controller.set_fan(True)
            buzzer_controller.beep(3, 200, 100)  # 3 long beeps = fan starting
            print(f"🌀 [FAN] Starting - CPU at {cpu_temp:.1f}°C (threshold: {FAN_ON_THRESHOLD}°C)")
        elif cpu_temp <= FAN_OFF_THRESHOLD and current_fan_state:
            fan_controller.set_fan(False)
            buzzer_controller.beep(2, 150, 150)  # 2 short beeps = fan stopping
            print(f"🌀 [FAN] Stopping - CPU cooled to {cpu_temp:.1f}°C (threshold: {FAN_OFF_THRESHOLD}°C)")
        
        # Get updated fan state after potential change
        fan_on = fan_controller.get_fan_state()
        
        # LED 3 for CPU temp
        if cpu_temp > 75.0:
            led_controller.set_led(3, 255, 0, 0)  # Red - critical
            buzzer_controller.beep(2, 50, 50)
            print(f"🔴 [PI4] CRITICAL: {cpu_temp:.1f}°C | Fan: {'ON' if fan_on else 'OFF'}")
        elif cpu_temp > 60.0:
            led_controller.set_led(3, 255, 100, 0)  # Orange - warm
            print(f"🟠 [PI4] Warm: {cpu_temp:.1f}°C | Fan: {'ON' if fan_on else 'OFF'}")
        else:
            led_controller.set_led(3, 0, 255, 0)  # Green - OK
            print(f"🟢 [PI4] OK: {cpu_temp:.1f}°C | Fan: {'ON' if fan_on else 'OFF'}")
        
        led_controller.sync_leds()
        
        return PiStats(
            cpu_temp=cpu_temp,
            cpu_usage=cpu_usage,
            memory_used_mb=mem_used,
            memory_total_mb=mem_total,
            uptime_seconds=uptime,
            timestamp_ms=timestamp,
            fan_on=fan_on
        )
