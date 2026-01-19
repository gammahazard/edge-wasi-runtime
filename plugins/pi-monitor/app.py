"""
==============================================================================
app.py - Raspberry Pi system monitoring plugin
==============================================================================

purpose:
    this module implements the pi-monitor-logic interface defined in plugin.wit.
    it reads CPU temperature and system stats, controlling LED 0.

the monitoring thresholds in this file are HOT-SWAPPABLE:
    - change CPU temp warning levels
    - change LED colors
    - rebuild wasm, host auto-reloads without restart

relations:
    - implements: ../../wit/plugin.wit (pi-monitor-logic)
    - imports: gpio-provider, led-controller (from rust host)
    - loaded by: ../../host/src/runtime.rs
    - called by: ../../host/src/main.rs (polling loop)

build command:
    componentize-py -d ../../wit -w pi-monitor-plugin componentize app -o pi-monitor.wasm

==============================================================================
"""

from wit_world.exports import PiMonitorLogic
from wit_world.exports.pi_monitor_logic import PiStats
from wit_world.imports import gpio_provider, led_controller


# ==============================================================================
# CPU temperature thresholds
# ==============================================================================
CPU_TEMP_HOT = 70.0     # Red - danger
CPU_TEMP_WARM = 50.0    # Yellow - warning


class PiMonitorLogic(PiMonitorLogic):
    """
    Implementation of the pi-monitor-logic interface from plugin.wit.
    Controls LED 0 for CPU/system health status.
    """
    
    def poll(self) -> PiStats:
        """
        Poll Pi system stats and update LED 0.
        """
        timestamp_ms = gpio_provider.get_timestamp_ms()
        
        # Get CPU temperature via host capability
        cpu_temp = gpio_provider.get_cpu_temp()
        
        # Control LED 0 based on CPU temp
        if cpu_temp > CPU_TEMP_HOT:
            led_controller.set_led(0, 255, 0, 0)  # Red - HOT
            print(f"🔴 [PI] CPU HOT: {cpu_temp:.1f}°C")
        elif cpu_temp > CPU_TEMP_WARM:
            led_controller.set_led(0, 255, 255, 0)  # Yellow - warm
            print(f"🟡 [PI] CPU Warm: {cpu_temp:.1f}°C")
        else:
            led_controller.set_led(0, 0, 255, 0)  # Green - cool
            print(f"🟢 [PI] CPU OK: {cpu_temp:.1f}°C")
        
        # Return stats
        # Note: memory/cpu-usage not yet implemented in host gpio-provider
        # Future enhancement: add get-memory-info() and get-cpu-usage() to WIT
        return PiStats(
            cpu_temp=cpu_temp,
            cpu_usage=0.0,  # TODO: implement in host
            memory_used_mb=0,  # TODO: implement in host
            memory_total_mb=0,  # TODO: implement in host
            uptime_seconds=0,  # TODO: implement in host
            timestamp_ms=timestamp_ms
        )
