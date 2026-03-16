const net = require('net');
const WebSocket = require('ws');

const FLUXDB_HOST = '127.0.0.1';
const FLUXDB_PORT = 7000;
const WS_PORT = 8080;

// Create WebSocket server
const wss = new WebSocket.Server({ port: WS_PORT });

console.log(`WebSocket bridge server running on ws://localhost:${WS_PORT}`);

wss.on('connection', (ws) => {
    console.log('Client connected');
    
    // Create TCP connection to fluxdb for each WebSocket client
    let tcpClient = null;
    const requestQueue = [];
    
    function connectToFluxdb() {
        tcpClient = net.createConnection({ host: FLUXDB_HOST, port: FLUXDB_PORT }, () => {
            console.log('Connected to fluxdb');
        });
        
        tcpClient.on('data', (data) => {
            const lines = data.toString().split('\n').filter(line => line.trim());
            
            for (const line of lines) {
                try {
                    const response = JSON.parse(line);
                    console.log('FluxDB response:', response);
                    
                    if (response.kind === 'event') {
                        // This is a subscription event, forward to all clients
                        ws.send(JSON.stringify({
                            type: 'event',
                            data: response
                        }));
                    } else {
                        // Response to a command
                        if (requestQueue.length > 0) {
                            const callback = requestQueue.shift();
                            callback(response);
                        }
                    }
                } catch (e) {
                    console.error('Error parsing response:', e);
                }
            }
        });
        
        tcpClient.on('error', (err) => {
            console.error('TCP error:', err);
            ws.send(JSON.stringify({
                type: 'error',
                message: 'Database connection error'
            }));
        });
        
        tcpClient.on('close', () => {
            console.log('Disconnected from fluxdb');
        });
    }
    
    function sendToFluxdb(request, callback) {
        if (!tcpClient || !tcpClient.writable) {
            callback({ kind: 'error', message: 'Not connected to database' });
            return;
        }
        
        if (callback) {
            requestQueue.push(callback);
        }
        
        tcpClient.write(JSON.stringify(request) + '\n');
    }
    
    // Connect to fluxdb
    connectToFluxdb();
    
    // Handle WebSocket messages
    ws.on('message', (message) => {
        try {
            const msg = JSON.parse(message);
            
            switch (msg.type) {
                case 'get':
                    sendToFluxdb({ kind: 'get', key: msg.key }, (response) => {
                        console.log('Get response:', response);
                        ws.send(JSON.stringify({
                            type: 'get_response',
                            key: msg.key,
                            data: response
                        }));
                    });
                    break;
                    
                case 'set':
                    sendToFluxdb({ kind: 'set', key: msg.key, value: msg.value }, (response) => {
                        ws.send(JSON.stringify({
                            type: 'set_response',
                            key: msg.key,
                            data: response
                        }));
                    });
                    break;
                    
                case 'delete':
                    sendToFluxdb({ kind: 'del', key: msg.key }, (response) => {
                        ws.send(JSON.stringify({
                            type: 'delete_response',
                            key: msg.key,
                            data: response
                        }));
                    });
                    break;
                    
                case 'patch':
                    sendToFluxdb({ kind: 'patch', key: msg.key, delta: msg.delta }, (response) => {
                        ws.send(JSON.stringify({
                            type: 'patch_response',
                            key: msg.key,
                            data: response
                        }));
                    });
                    break;
                    
                case 'subscribe':
                    sendToFluxdb({ kind: 'subscribe', key: msg.key }, (response) => {
                        console.log('Subscribe response:', response);
                        // Forward the subscribed response to client
                        ws.send(JSON.stringify({
                            type: 'subscribe_response',
                            key: msg.key,
                            data: response
                        }));
                    });
                    break;
                    
                default:
                    ws.send(JSON.stringify({
                        type: 'error',
                        message: 'Unknown message type'
                    }));
            }
        } catch (e) {
            ws.send(JSON.stringify({
                type: 'error',
                message: 'Invalid JSON: ' + e.message
            }));
        }
    });
    
    ws.on('close', () => {
        console.log('Client disconnected');
        if (tcpClient) {
            tcpClient.end();
        }
    });
    
    ws.on('error', (err) => {
        console.error('WebSocket error:', err);
    });
});

console.log(`FluxDB Real-time Demo Bridge Server`);
console.log(`====================================`);
console.log(`WebSocket: ws://localhost:${WS_PORT}`);
console.log(`FluxDB TCP:  ${FLUXDB_HOST}:${FLUXDB_PORT}`);
