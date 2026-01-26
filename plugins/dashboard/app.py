"""
==============================================================================
dashboard_plugin.py - WASM Rendered Harvester Dashboard
==============================================================================
"""
import json
from wit_world.exports import DashboardLogic

class DashboardLogic(DashboardLogic):
    def render(self, sensor_data: str) -> str:
        try:
            state = json.loads(sensor_data)
        except:
            return "<h1>Error: Invalid State JSON</h1>"

        readings = state.get("readings", [])
        
        # Premium Design System
        html = """
        <!DOCTYPE html>
        <html lang="en">
        <head>
            <meta charset="UTF-8">
            <meta name="viewport" content="width=device-width, initial-scale=1.0">
            <title>Harvester OS | Industrial Telemetry</title>
            <link rel="preconnect" href="https://fonts.googleapis.com">
            <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
            <link href="https://fonts.googleapis.com/css2?family=Outfit:wght@300;400;600&family=JetBrains+Mono&display=swap" rel="stylesheet">
            <style>
                :root {
                    --bg-color: #050507;
                    --glass-bg: rgba(26, 26, 30, 0.7);
                    --glass-border: rgba(255, 255, 255, 0.1);
                    --primary: #00f2ff;
                    --secondary: #ff9d00;
                    --accent: #8c00ff;
                    --text-main: #e0e0e0;
                    --text-dim: #909090;
                    --success: #00ff88;
                }

                * { box-sizing: border-box; }
                body {
                    background-color: var(--bg-color);
                    background-image: 
                        radial-gradient(circle at 10% 20%, rgba(0, 242, 255, 0.05) 0%, transparent 40%),
                        radial-gradient(circle at 90% 80%, rgba(140, 0, 255, 0.05) 0%, transparent 40%);
                    color: var(--text-main);
                    font-family: 'Outfit', sans-serif;
                    margin: 0;
                    padding: 2rem;
                    min-height: 100vh;
                }

                header {
                    display: flex;
                    justify-content: space-between;
                    align-items: center;
                    margin-bottom: 2rem;
                    padding: 1.5rem;
                    background: var(--glass-bg);
                    backdrop-filter: blur(12px);
                    border: 1px solid var(--glass-border);
                    border-radius: 20px;
                }

                h1 {
                    margin: 0;
                    font-weight: 600;
                    font-size: 1.8rem;
                    letter-spacing: -1px;
                    background: linear-gradient(90deg, var(--primary), #fff);
                    -webkit-background-clip: text;
                    -webkit-text-fill-color: transparent;
                }

                .status-badge {
                    background: rgba(0, 255, 136, 0.1);
                    color: var(--success);
                    padding: 0.5rem 1rem;
                    border-radius: 100px;
                    font-size: 0.9rem;
                    font-weight: 600;
                    border: 1px solid rgba(0, 255, 136, 0.2);
                    display: flex;
                    align-items: center;
                    gap: 0.5rem;
                }

                .status-dot {
                    width: 8px;
                    height: 8px;
                    background: var(--success);
                    border-radius: 50%;
                    box-shadow: 0 0 10px var(--success);
                    animation: pulse 2s infinite;
                }

                @keyframes pulse {
                    0% { opacity: 1; }
                    50% { opacity: 0.4; }
                    100% { opacity: 1; }
                }

                .fleet-grid {
                    display: grid;
                    grid-template-columns: repeat(auto-fit, minmax(350px, 1fr));
                    gap: 1.5rem;
                }

                .node-card {
                    background: var(--glass-bg);
                    backdrop-filter: blur(24px);
                    border: 1px solid var(--glass-border);
                    border-radius: 24px;
                    padding: 1.5rem;
                    transition: transform 0.3s ease, border-color 0.3s ease;
                    position: relative;
                    overflow: hidden;
                }

                .node-card::before {
                    content: '';
                    position: absolute;
                    top: 0; left: 0; right: 0; height: 1px;
                    background: linear-gradient(90deg, transparent, rgba(255,255,255,0.2), transparent);
                }

                .node-header {
                    display: flex;
                    justify-content: space-between;
                    align-items: flex-start;
                    margin-bottom: 1.5rem;
                }

                .node-id {
                    font-size: 1.2rem;
                    font-weight: 600;
                    color: #fff;
                }

                .node-type {
                    font-size: 0.7rem;
                    text-transform: uppercase;
                    letter-spacing: 1px;
                    color: var(--text-dim);
                    background: rgba(255, 255, 255, 0.05);
                    padding: 4px 8px;
                    border-radius: 6px;
                }

                .stat-grid {
                    display: grid;
                    grid-template-columns: 1fr 1fr;
                    gap: 1rem;
                }

                .stat-box {
                    background: rgba(255, 255, 255, 0.03);
                    padding: 1rem;
                    border-radius: 16px;
                    border: 1px solid rgba(255, 255, 255, 0.05);
                    transition: background 0.5s ease;
                }

                .stat-label {
                    font-size: 0.75rem;
                    color: var(--text-dim);
                    margin-bottom: 0.3rem;
                    text-transform: uppercase;
                    letter-spacing: 0.5px;
                }

                .stat-value {
                    font-family: 'JetBrains Mono', monospace;
                    font-size: 1.25rem;
                    font-weight: 600;
                    color: var(--primary);
                    transition: opacity 0.3s ease, transform 0.3s ease;
                }

                .updating {
                    opacity: 0.4;
                    transform: translateY(-2px);
                }

                .temp-hot { color: #ff4c4c; }
                .temp-warm { color: #ff9d00; }
                .temp-cool { color: #00f2ff; }

                .raw-data-panel {
                    margin-top: 1.5rem;
                    padding: 1rem;
                    background: rgba(0, 0, 0, 0.3);
                    border-radius: 12px;
                    font-family: 'JetBrains Mono', monospace;
                    font-size: 0.7rem;
                    color: #505050;
                    max-height: 80px;
                    overflow-y: auto;
                    display: none;
                }
            </style>
        </head>
        <body>
            <header>
                <h1>HARVESTER OS <span style="font-weight: 300; opacity: 0.5;">v2.0.0</span></h1>
                <div class="status-badge">
                    <div class="status-dot"></div>
                    <span id="system-status">LIVE CLUSTER FEED</span>
                </div>
            </header>

            <div class="fleet-grid" id="fleet-grid">
                <!-- Dynamically Injected Cards -->
            </div>

            <script>
                const grid = document.getElementById('fleet-grid');
                let lastData = null;

                async function updateDashboard() {
                    try {
                        const response = await fetch('/api/readings');
                        const state = await response.json();
                        const readings = state.readings || [];
                        
                        // Group by node
                        const nodes = {};
                        readings.forEach(r => {
                            const nodeName = r.sensor_id.split(':')[0] || 'HUB';
                            if (!nodes[nodeName]) nodes[nodeName] = [];
                            nodes[nodeName].push(r);
                        });

                        // Clear if no data
                        if (Object.keys(nodes).length === 0) {
                            grid.innerHTML = '<div class="node-card" style="grid-column: 1/-1; text-align: center; padding: 4rem;">Initializing...</div>';
                            return;
                        }

                        // Generate cards
                        let newHtml = '';
                        for (const [nodeName, nodeReadings] of Object.entries(nodes)) {
                            const isHub = nodeName.toLowerCase().includes('hub');
                            let sensorHtml = '';
                            
                            nodeReadings.forEach(r => {
                                const sensorType = r.sensor_id.split(':').pop().toLowerCase();
                                const data = r.data || {};
                                
                                if (sensorType.includes('dht22')) {
                                    sensorHtml += `
                                        <div class="stat-box" style="grid-column: 1/-1; border-left: 2px solid var(--primary);">
                                            <div class="stat-label">DHT22 SENSOR</div>
                                            <div style="display:flex; justify-content:space-between;">
                                                <div><span class="stat-label">TEMP</span> <div class="stat-value" id="${nodeName}-dht-t">${data.temperature?.toFixed(1)}°C</div></div>
                                                <div><span class="stat-label">HUM</span> <div class="stat-value" id="${nodeName}-dht-h">${data.humidity?.toFixed(1)}%</div></div>
                                            </div>
                                        </div>`;
                                } else if (sensorType.includes('bme680')) {
                                    sensorHtml += `
                                        <div class="stat-box" style="grid-column: 1/-1; border-left: 2px solid var(--secondary);">
                                            <div class="stat-label">BME680 SENSOR</div>
                                            <div style="display:flex; justify-content:space-between;">
                                                <div><span class="stat-label">AIR</span> <div class="stat-value" id="${nodeName}-bme-t">${data.temperature?.toFixed(1)}°C</div></div>
                                                <div><span class="stat-label">IAQ</span> <div class="stat-value" id="${nodeName}-bme-iaq">${data.iaq_score || 0}</div></div>
                                            </div>
                                        </div>`;
                                } else if (sensorType.includes('monitor')) {
                                    const t = data.cpu_temp || 0;
                                    const tClass = t > 65 ? 'temp-hot' : (t > 50 ? 'temp-warm' : 'temp-cool');
                                    sensorHtml += `
                                        <div class="stat-box">
                                            <div class="stat-label">CPU TEMP</div>
                                            <div class="stat-value ${tClass}" id="${nodeName}-cpu-t">${t.toFixed(1)}°C</div>
                                        </div>
                                        <div class="stat-box">
                                            <div class="stat-label">CPU LOAD</div>
                                            <div class="stat-value" id="${nodeName}-cpu-l">${data.cpu_usage?.toFixed(1)}%</div>
                                        </div>`;
                                }
                            });

                            newHtml += `
                                <div class="node-card" id="card-${nodeName}">
                                    <div class="node-header">
                                        <div>
                                            <div class="node-id">${nodeName.toUpperCase()}</div>
                                            <div class="node-type">${isHub ? 'CORE COMMAND' : 'HARVESTER SPOKE'}</div>
                                        </div>
                                        <div style="color: ${isHub ? 'var(--primary)' : 'var(--secondary)'}; font-size: 1.5rem;">
                                            ${isHub ? '🏰' : '🚜'}
                                        </div>
                                    </div>
                                    <div class="stat-grid">${sensorHtml}</div>
                                </div>`;
                        }

                        // Only replace grid if structure changed, otherwise update values for animations
                        if (grid.children.length !== Object.keys(nodes).length) {
                             grid.innerHTML = newHtml;
                        } else {
                            // Update values with fade effect
                            for (const [nodeName, nodeReadings] of Object.entries(nodes)) {
                                nodeReadings.forEach(r => {
                                    const sensorType = r.sensor_id.split(':').pop().toLowerCase();
                                    const data = r.data || {};
                                    if (sensorType.includes('dht22')) {
                                        updateValue(`${nodeName}-dht-t`, data.temperature?.toFixed(1) + "°C");
                                        updateValue(`${nodeName}-dht-h`, data.humidity?.toFixed(1) + "%");
                                    } else if (sensorType.includes('bme680')) {
                                        updateValue(`${nodeName}-bme-t`, data.temperature?.toFixed(1) + "°C");
                                        updateValue(`${nodeName}-bme-iaq`, data.iaq_score || 0);
                                    } else if (sensorType.includes('monitor')) {
                                        updateValue(`${nodeName}-cpu-t`, data.cpu_temp?.toFixed(1) + "°C");
                                        updateValue(`${nodeName}-cpu-l`, data.cpu_usage?.toFixed(1) + "%");
                                    }
                                });
                            }
                        }
                    } catch (e) {
                        console.error("Dashboard Sync Error:", e);
                        document.getElementById('system-status').innerText = 'LINK INTERRUPTED';
                        document.getElementById('system-status').style.color = '#ff4c4c';
                    }
                }

                function updateValue(id, newValue) {
                    const el = document.getElementById(id);
                    if (el && el.innerText != newValue) {
                        el.classList.add('updating');
                        setTimeout(() => {
                            el.innerText = newValue;
                            el.classList.remove('updating');
                        }, 300);
                    }
                }

                setInterval(updateDashboard, 2000);
                updateDashboard();
            </script>
        </body>
        </html>
        """
        return html
