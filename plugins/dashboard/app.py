"""
==============================================================================
dashboard_plugin.py - Harvester OS Dashboard
==============================================================================

Multi-node sensor dashboard with:
- Real-time sensor data (DHT22, BME680)
- System health for Hub, Pi4 Spoke, PiZero nodes
- Buzzer controls
- Live log viewer
- Dynamic JavaScript updates

Build:
    componentize-py -d ../../wit -w dashboard-plugin componentize app -o dashboard.wasm
"""

from wit_world.exports import DashboardLogic
import json


class DashboardLogic(DashboardLogic):
    def render(self, sensor_data: str) -> str:
        """Render the HTML dashboard with sensor data JSON."""
        data = json.loads(sensor_data) if sensor_data else {}
        
        # Extract values with defaults
        dht = data.get("dht22", {})
        dht_temp = dht.get("temperature", 0.0)
        dht_hum = dht.get("humidity", 0.0)
        
        bme = data.get("bme680", {})
        bme_temp = bme.get("temperature", 0.0)
        bme_hum = bme.get("humidity", 0.0)
        pressure = bme.get("pressure", 0.0)
        gas = bme.get("gas_resistance", 0.0)
        iaq = bme.get("iaq_score", 0)
        
        # Hub (RevPi) Data
        pi_hub = data.get("pi", {})
        hub_cpu = pi_hub.get("cpu_temp", 0.0)
        hub_ram_used = pi_hub.get("memory_used_mb", 0)
        hub_ram_total = pi_hub.get("memory_total_mb", 0)
        hub_uptime = pi_hub.get("uptime_seconds", 0)
        
        # Spoke Pi4 Data
        pi4 = data.get("pi4", {})
        pi4_cpu = pi4.get("cpu_temp", 0.0)
        pi4_ram_used = pi4.get("memory_used_mb", 0)
        pi4_ram_total = pi4.get("memory_total_mb", 0)
        
        # PiZero Data
        pizero = data.get("pizero", {})
        pizero_cpu = pizero.get("cpu_temp", 0.0)
        pizero_ram_used = pizero.get("memory_used_mb", 0)
        pizero_ram_total = pizero.get("memory_total_mb", 0)
        pizero_online = "cpu_temp" in pizero
        
        # IAQ Text
        if iaq == 0:
            iaq_text = "CALIB..."
            iaq_class = "calib"
        elif iaq <= 50:
            iaq_text = "EXCELLENT"
            iaq_class = "excellent"
        elif iaq <= 100:
            iaq_text = "GOOD"
            iaq_class = "good"
        elif iaq <= 150:
            iaq_text = "MODERATE"
            iaq_class = "moderate"
        elif iaq <= 200:
            iaq_text = "POOR"
            iaq_class = "poor"
        else:
            iaq_text = "BAD"
            iaq_class = "bad"
        
        # Uptime
        up_h = hub_uptime // 3600
        up_m = (hub_uptime % 3600) // 60
        uptime_str = f"{up_h}h {up_m}m"
        
        return f'''<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>HARVESTER OS</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link href="https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;700&display=swap" rel="stylesheet">
    <style>
        :root {{
            --bg: #0a0a0f;
            --card: #12121a;
            --border: #2a2a3a;
            --green: #00ff88;
            --red: #ff4444;
            --yellow: #ffcc00;
            --blue: #00aaff;
            --purple: #aa66ff;
            --dim: #666;
        }}
        * {{ box-sizing: border-box; margin: 0; padding: 0; }}
        body {{
            font-family: 'JetBrains Mono', monospace;
            background: var(--bg);
            color: #eee;
            min-height: 100vh;
            padding: 20px;
        }}
        .header {{
            text-align: center;
            border-bottom: 1px solid var(--border);
            padding-bottom: 15px;
            margin-bottom: 20px;
        }}
        h1 {{ color: var(--green); font-size: 2rem; letter-spacing: 3px; }}
        .subtitle {{ color: var(--dim); font-size: 0.9rem; margin-top: 5px; }}
        .grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
            gap: 15px;
            max-width: 1200px;
            margin: 0 auto;
        }}
        .card {{
            background: var(--card);
            border: 1px solid var(--border);
            border-radius: 8px;
            padding: 15px;
        }}
        .card-title {{
            color: var(--green);
            font-size: 0.85rem;
            border-bottom: 1px dashed var(--border);
            padding-bottom: 8px;
            margin-bottom: 10px;
        }}
        .card-title.warn {{ color: var(--yellow); }}
        .card-title.danger {{ color: var(--red); }}
        .value {{ font-size: 2.5rem; font-weight: 700; }}
        .unit {{ font-size: 1rem; opacity: 0.6; }}
        .metrics {{ display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin-top: 10px; font-size: 0.8rem; }}
        .metric span {{ opacity: 0.5; display: block; }}
        .iaq {{ padding: 5px 10px; border-radius: 4px; display: inline-block; margin-top: 5px; }}
        .iaq.excellent {{ background: var(--green); color: #000; }}
        .iaq.good {{ background: #88ff88; color: #000; }}
        .iaq.moderate {{ background: var(--yellow); color: #000; }}
        .iaq.poor {{ background: #ff8800; color: #000; }}
        .iaq.bad {{ background: var(--red); color: #fff; }}
        .iaq.calib {{ background: var(--purple); color: #fff; }}
        .controls {{
            max-width: 1200px;
            margin: 20px auto;
            display: flex;
            gap: 10px;
            flex-wrap: wrap;
            justify-content: center;
        }}
        .btn {{
            background: transparent;
            color: var(--green);
            border: 1px solid var(--green);
            padding: 10px 20px;
            font-family: inherit;
            cursor: pointer;
            border-radius: 4px;
            transition: all 0.2s;
        }}
        .btn:hover {{ background: var(--green); color: #000; }}
        .logs {{
            max-width: 1200px;
            margin: 20px auto;
            background: var(--card);
            border: 1px solid var(--border);
            border-radius: 8px;
            padding: 15px;
            max-height: 300px;
            overflow-y: auto;
        }}
        .logs h3 {{ color: var(--green); margin-bottom: 10px; font-size: 0.9rem; }}
        .log-line {{ font-size: 0.75rem; color: var(--dim); padding: 2px 0; }}
        .tabs {{ display: flex; gap: 10px; margin-bottom: 10px; }}
        .tab {{ cursor: pointer; padding: 5px 15px; border-radius: 4px; }}
        .tab.active {{ background: var(--green); color: #000; }}
        .tab:not(.active) {{ border: 1px solid var(--border); }}
        .node-status {{ display: flex; align-items: center; gap: 8px; }}
        .dot {{ width: 8px; height: 8px; border-radius: 50%; }}
        .dot.online {{ background: var(--green); }}
        .dot.offline {{ background: var(--red); }}
    </style>
</head>
<body>
    <div class="header">
        <h1>▲ HARVESTER OS</h1>
        <div class="subtitle">UPTIME: {uptime_str} | NODES: 3</div>
    </div>
    
    <div class="grid">
        <div class="card">
            <div class="card-title">DHT22 [ROOM]</div>
            <div class="value">{dht_temp:.1f}<span class="unit">°C</span></div>
            <div class="metrics">
                <div class="metric"><span>HUMIDITY</span>{dht_hum:.0f}%</div>
            </div>
        </div>
        
        <div class="card">
            <div class="card-title">BME680 [AIR]</div>
            <div class="value">{bme_temp:.1f}<span class="unit">°C</span></div>
            <div class="metrics">
                <div class="metric"><span>HUMIDITY</span>{bme_hum:.0f}%</div>
                <div class="metric"><span>PRESSURE</span>{pressure:.0f}hPa</div>
                <div class="metric"><span>GAS</span>{gas:.0f}KΩ</div>
                <div class="metric"><span>IAQ</span><span class="iaq {iaq_class}">{iaq} {iaq_text}</span></div>
            </div>
        </div>
        
        <div class="card">
            <div class="card-title {'warn' if hub_cpu > 60 else ''}">REVPI HUB</div>
            <div class="value">{hub_cpu:.1f}<span class="unit">°C</span></div>
            <div class="metrics">
                <div class="metric"><span>RAM</span>{hub_ram_used}/{hub_ram_total}MB</div>
            </div>
        </div>
        
        <div class="card">
            <div class="card-title {'warn' if pi4_cpu > 60 else ''}">PI4 SPOKE</div>
            <div class="value">{pi4_cpu:.1f}<span class="unit">°C</span></div>
            <div class="metrics">
                <div class="metric"><span>RAM</span>{pi4_ram_used}/{pi4_ram_total}MB</div>
            </div>
        </div>
        
        <div class="card">
            <div class="card-title">PIZERO <span class="node-status"><span class="dot {'online' if pizero_online else 'offline'}"></span>{'ONLINE' if pizero_online else 'OFFLINE'}</span></div>
            <div class="value">{pizero_cpu:.1f}<span class="unit">°C</span></div>
            <div class="metrics">
                <div class="metric"><span>RAM</span>{pizero_ram_used}/{pizero_ram_total}MB</div>
            </div>
        </div>
    </div>
    
    <div class="controls">
        <button class="btn" onclick="buzzer('beep')">[ BEEP ]</button>
        <button class="btn" onclick="buzzer('beep3')">[ BEEP x3 ]</button>
        <button class="btn" onclick="buzzer('long')">[ LONG ]</button>
    </div>
    
    <div class="logs" id="logs">
        <div class="tabs">
            <div class="tab active" onclick="switchLogs('hub')">HUB</div>
            <div class="tab" onclick="switchLogs('pi4')">PI4</div>
            <div class="tab" onclick="switchLogs('pizero')">PIZERO</div>
        </div>
        <div id="log-content"></div>
    </div>
    
    <script>
        let currentNode = 'hub';
        const logUrls = {{
            hub: '/api/logs',
            pi4: 'http://192.168.7.11:3000/api/logs',
            pizero: 'http://192.168.7.12:3000/api/logs'
        }};
        
        async function buzzer(action) {{
            await fetch('/api/buzzer?action=' + action, {{method: 'POST'}});
        }}
        
        function switchLogs(node) {{
            currentNode = node;
            document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
            event.target.classList.add('active');
            fetchLogs();
        }}
        
        async function fetchLogs() {{
            try {{
                const res = await fetch(logUrls[currentNode]);
                const data = await res.json();
                const html = (data.logs || []).map(l => '<div class="log-line">' + l + '</div>').join('');
                document.getElementById('log-content').innerHTML = html || '<div class="log-line">No logs</div>';
            }} catch(e) {{
                document.getElementById('log-content').innerHTML = '<div class="log-line">Failed to fetch logs</div>';
            }}
        }}
        
        fetchLogs();
        setInterval(fetchLogs, 3000);
        setInterval(() => location.reload(), 10000);
    </script>
</body>
</html>'''
