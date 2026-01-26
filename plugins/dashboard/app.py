"""
==============================================================================
dashboard/app.py - Harvester OS Multi-Node Dashboard
==============================================================================

Features:
- 5 sensor cards: DHT22 ROOM, BME680 AIR, REVPI HUB, PI4 SPOKE, PIZERO SPOKE
- Log viewer with tabs for HUB/PI4/PIZERO
- Buzzer controls (BEEP, BEEP x3, LONG)
- JetBrains Mono terminal aesthetic
- Auto-refresh every 10 seconds

Build:
    componentize-py -d ../../wit -w dashboard-plugin componentize app -o dashboard.wasm
==============================================================================
"""
import json
from wit_world.exports import DashboardLogic


class DashboardLogic(DashboardLogic):
    def render(self, sensor_data: str) -> str:
        try:
            state = json.loads(sensor_data)
        except:
            state = {}
        
        # Extract sensor data with defaults
        dht = state.get("dht22", {})
        bme = state.get("bme680", {})
        hub = state.get("hub", {})
        pi4 = state.get("pi4", {})
        pizero = state.get("pizero", {})
        
        dht_temp = dht.get("temperature", 0.0)
        dht_hum = dht.get("humidity", 0.0)
        
        bme_temp = bme.get("temperature", 0.0)
        bme_hum = bme.get("humidity", 0.0)
        pressure = bme.get("pressure", 0.0)
        gas = bme.get("gas_resistance", 0.0)
        iaq = bme.get("iaq_score", 0)
        
        hub_cpu = hub.get("cpu_temp", 0.0)
        hub_load = hub.get("cpu_usage", 0.0)
        hub_ram_used = hub.get("memory_used_mb", 0)
        hub_ram_total = hub.get("memory_total_mb", 0)
        hub_uptime = hub.get("uptime_seconds", 0)
        
        pi4_cpu = pi4.get("cpu_temp", 0.0)
        pi4_load = pi4.get("cpu_usage", 0.0)
        pi4_ram_used = pi4.get("memory_used_mb", 0)
        pi4_ram_total = pi4.get("memory_total_mb", 0)
        
        pizero_cpu = pizero.get("cpu_temp", 0.0)
        pizero_load = pizero.get("cpu_usage", 0.0)
        pizero_ram_used = pizero.get("memory_used_mb", 0)
        pizero_ram_total = pizero.get("memory_total_mb", 0)
        pizero_online = pizero.get("online", False)
        
        # Network health from PiZero pings
        network = state.get("network", {})
        hub_ping = network.get("192.168.7.10", -1)
        pi4_ping = network.get("192.168.7.11", -1)
        
        # IAQ classification
        if iaq <= 50:
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
        
        # Uptime string
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
            --text: #e0e0e0;
            --dim: #666;
        }}
        * {{ box-sizing: border-box; margin: 0; padding: 0; }}
        body {{
            background: var(--bg);
            color: var(--text);
            font-family: 'JetBrains Mono', monospace;
            padding: 1.5rem;
            min-height: 100vh;
        }}
        header {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 1.5rem;
            padding-bottom: 1rem;
            border-bottom: 1px solid var(--border);
        }}
        h1 {{
            font-size: 1.5rem;
            color: var(--green);
            text-shadow: 0 0 20px var(--green);
        }}
        .uptime {{ color: var(--dim); font-size: 0.85rem; }}
        .grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
            gap: 1rem;
            margin-bottom: 1.5rem;
        }}
        .card {{
            background: var(--card);
            border: 1px solid var(--border);
            border-radius: 8px;
            padding: 1.25rem;
        }}
        .card-title {{
            font-size: 0.75rem;
            color: var(--dim);
            margin-bottom: 0.75rem;
            letter-spacing: 1px;
        }}
        .card-title.warn {{ color: var(--yellow); }}
        .value {{
            font-size: 2.5rem;
            font-weight: 700;
            color: var(--green);
        }}
        .unit {{ font-size: 1rem; color: var(--dim); }}
        .metrics {{
            display: flex;
            flex-wrap: wrap;
            gap: 1rem;
            margin-top: 1rem;
            padding-top: 0.75rem;
            border-top: 1px solid var(--border);
        }}
        .metric {{
            font-size: 0.8rem;
        }}
        .metric span {{ color: var(--dim); margin-right: 0.5rem; }}
        .iaq {{
            padding: 0.2rem 0.5rem;
            border-radius: 4px;
            font-weight: 700;
        }}
        .iaq.excellent {{ background: #00ff8833; color: var(--green); }}
        .iaq.good {{ background: #00aaff33; color: var(--blue); }}
        .iaq.moderate {{ background: #ffcc0033; color: var(--yellow); }}
        .iaq.poor {{ background: #ff990033; color: #ff9900; }}
        .iaq.bad {{ background: #ff444433; color: var(--red); }}
        .node-status {{
            font-size: 0.7rem;
            margin-left: 0.5rem;
        }}
        .dot {{
            display: inline-block;
            width: 8px;
            height: 8px;
            border-radius: 50%;
            margin-right: 4px;
        }}
        .dot.online {{ background: var(--green); box-shadow: 0 0 6px var(--green); }}
        .dot.offline {{ background: var(--red); }}
        .controls {{
            display: flex;
            gap: 1rem;
            margin-bottom: 1.5rem;
        }}
        .btn {{
            background: var(--card);
            border: 1px solid var(--green);
            color: var(--green);
            padding: 0.75rem 1.5rem;
            border-radius: 4px;
            cursor: pointer;
            font-family: inherit;
            font-size: 0.85rem;
            transition: all 0.2s;
        }}
        .btn:hover {{
            background: var(--green);
            color: var(--bg);
        }}
        .logs {{
            background: var(--card);
            border: 1px solid var(--border);
            border-radius: 8px;
            overflow: hidden;
        }}
        .tabs {{
            display: flex;
            border-bottom: 1px solid var(--border);
        }}
        .tab {{
            padding: 0.75rem 1.5rem;
            cursor: pointer;
            color: var(--dim);
            border-bottom: 2px solid transparent;
            transition: all 0.2s;
        }}
        .tab:hover {{ color: var(--text); }}
        .tab.active {{
            color: var(--green);
            border-bottom-color: var(--green);
        }}
        #log-content {{
            padding: 1rem;
            max-height: 300px;
            overflow-y: auto;
            font-size: 0.75rem;
        }}
        .log-line {{
            padding: 0.25rem 0;
            border-bottom: 1px solid var(--border);
            color: var(--dim);
        }}
    </style>
</head>
<body>
    <header>
        <h1>[ HARVESTER OS ]</h1>
        <div class="uptime">UPTIME: {uptime_str}</div>
    </header>
    
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
                <div class="metric"><span>CPU</span>{hub_load:.1f}%</div>
                <div class="metric"><span>RAM</span>{hub_ram_used}/{hub_ram_total}MB</div>
            </div>
        </div>
        
        <div class="card">
            <div class="card-title {'warn' if pi4_cpu > 60 else ''}">PI4 SPOKE</div>
            <div class="value">{pi4_cpu:.1f}<span class="unit">°C</span></div>
            <div class="metrics">
                <div class="metric"><span>CPU</span>{pi4_load:.1f}%</div>
                <div class="metric"><span>RAM</span>{pi4_ram_used}/{pi4_ram_total}MB</div>
            </div>
        </div>
        
        <div class="card">
            <div class="card-title">PIZERO <span class="node-status"><span class="dot {'online' if pizero_online else 'offline'}"></span>{'ONLINE' if pizero_online else 'OFFLINE'}</span></div>
            <div class="value">{pizero_cpu:.1f}<span class="unit">°C</span></div>
            <div class="metrics">
                <div class="metric"><span>CPU</span>{pizero_load:.1f}%</div>
                <div class="metric"><span>RAM</span>{pizero_ram_used}/{pizero_ram_total}MB</div>
            </div>
        </div>
        
        <div class="card">
            <div class="card-title">NETWORK [PING from PIZERO]</div>
            <div class="metrics" style="border-top: none; padding-top: 0;">
                <div class="metric"><span class="dot {'online' if hub_ping >= 0 else 'offline'}"></span><span>HUB</span>{f'{hub_ping:.1f}ms' if hub_ping >= 0 else 'OFFLINE'}</div>
                <div class="metric"><span class="dot {'online' if pi4_ping >= 0 else 'offline'}"></span><span>PI4</span>{f'{pi4_ping:.1f}ms' if pi4_ping >= 0 else 'OFFLINE'}</div>
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
