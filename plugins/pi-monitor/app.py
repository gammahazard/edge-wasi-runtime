"""
==============================================================================
app.py - Raspberry Pi system monitoring plugin
==============================================================================

purpose:
    this module implements the pi-monitor-logic interface defined in plugin.wit.
    it reads CPU temperature and system stats, controlling LED based on device.

led assignment:
    - Spoke (Pi 4, ~4GB RAM): LED 0
    - Hub (RevPi, ~8GB RAM): LED 3

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
from wit_world.imports import gpio_provider, led_controller, system_info


# ==============================================================================
# CPU temperature thresholds
# ==============================================================================
CPU_TEMP_HOT = 70.0     # Red - danger
CPU_TEMP_WARM = 50.0    # Yellow - warning

# RAM threshold to detect Hub vs Spoke
# RevPi Connect 4 has ~8GB RAM, Pi 4 has ~4GB
HUB_RAM_THRESHOLD_MB = 6000


class PiMonitorLogic(PiMonitorLogic):
    """
    Implementation of the pi-monitor-logic interface from plugin.wit.
    Controls LED 0 (Spoke) or LED 3 (Hub) for CPU/system health status.
    """
    
    def poll(self) -> PiStats:
        """
        Poll Pi system stats and update LED based on device type.
        """
        timestamp_ms = gpio_provider.get_timestamp_ms()
        
        # Get CPU temperature via host capability
        cpu_temp = gpio_provider.get_cpu_temp()
        
        # Get generic system stats via new capability
        cpu_usage = system_info.get_cpu_usage()
        used_mb, total_mb = system_info.get_memory_usage()
        uptime = system_info.get_uptime()
        
        # Auto-detect Hub vs Spoke based on RAM
        # Hub (RevPi) has ~8GB, Spoke (Pi 4) has ~4GB
        led_index = 3 if total_mb > HUB_RAM_THRESHOLD_MB else 0
        device_type = "HUB" if led_index == 3 else "PI"
        
        # Shared log string
        stats_msg = f"CPU: {cpu_temp:.1f}°C ({cpu_usage:.1f}%) | RAM: {used_mb}/{total_mb}MB | Up: {uptime}s"
        
        # Control LED based on CPU temp
        if cpu_temp > CPU_TEMP_HOT:
            led_controller.set_led(led_index, 255, 0, 0)  # Red - HOT
            print(f"🔴 [{device_type}] HOT | {stats_msg}")
        elif cpu_temp > CPU_TEMP_WARM:
            led_controller.set_led(led_index, 255, 255, 0)  # Yellow - warm
            print(f"🟡 [{device_type}] WARM | {stats_msg}")
        else:
            led_controller.set_led(led_index, 0, 255, 0)  # Green - cool
            print(f"🟢 [{device_type}] OK | {stats_msg}")
        
        return PiStats(
            cpu_temp=cpu_temp,
            cpu_usage=cpu_usage,
            memory_used_mb=used_mb,
            memory_total_mb=total_mb,
            uptime_seconds=uptime,
            timestamp_ms=timestamp_ms
        )
