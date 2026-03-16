# FluxDB Real-time Demo

This demo shows how FluxDB automatically pushes updates to connected web clients when data changes in the database.

## Features

- **Real-time Updates**: Watch data update automatically when the database changes
- **Subscribe to Keys**: Get live notifications when specific keys are modified
- **Auto-Update Counter**: See multiple clients stay in sync automatically
- **Event Logging**: Track all database operations in real-time

## Architecture

```
┌─────────────┐      ┌──────────────┐      ┌─────────────┐
│   Browser   │◄────►│  WebSocket   │◄────►│   FluxDB    │
│  (HTML/JS)  │ WebSocket│   Bridge   │  TCP  │   Server  │
└─────────────┘      └──────────────┘      └─────────────┘
     Port 3000            Port 8080            Port 7000
```

## Setup Instructions

### Step 1: Start FluxDB Server

First, make sure FluxDB is built and running:

```bash
# From the fluxdb project root
cargo build --release

# Start the fluxdb server
cargo run --bin server
```

The server will start listening on `127.0.0.1:7000`.

### Step 2: Install Node.js Dependencies

```bash
cd demo
npm install
```

### Step 3: Start the WebSocket Bridge

```bash
npm start
```

This starts the WebSocket bridge server on `ws://localhost:8080`.

### Step 4: Serve the Web Page

Open a new terminal and run:

```bash
npm run serve
```

This serves the web page on `http://localhost:3000`.

### Step 5: Open the Demo

Open your browser and navigate to `http://localhost:3000`.

## How to Use

### 1. Write Data

Enter a key and JSON value, then click **Set**:

- Key: `user:1`
- Value: `{"name": "John", "age": 30}`

### 2. Subscribe to Updates

Enter a key in the "Subscribe to Key" field and click **Subscribe**. You'll now see real-time updates whenever that key changes.

### 3. Watch the Auto-Update Counter

The counter demo shows real-time synchronization:

- Click **+1**, **+5**, or **-1** to modify the counter
- Open the page in multiple browser windows
- Changes in one window instantly appear in all others!

### 4. Patch Data

Use **Patch** to apply partial updates to existing data:

- Key: `user:1`
- Value: `{"age": 31}` (only updates the age field)

### 5. Delete Data

Select a key and click **Delete** to remove it from the database.

## Demo Scenarios

### Scenario 1: Multi-Window Sync

1. Open `http://localhost:3000` in two browser windows
2. In both windows, subscribe to the key `counter`
3. Click **+1** in one window
4. Watch both windows update automatically!

### Scenario 2: Live User Data

1. Set a user: `user:1` with value `{"name": "Alice", "status": "online"}`
2. Subscribe to `user:1`
3. In another window or using curl, update the user's status
4. Watch the data update in real-time

### Scenario 3: Patch Updates

1. Set `config:app` with `{"theme": "light", "lang": "en"}`
2. Subscribe to `config:app`
3. Patch with `{"theme": "dark"}`
4. See only the theme change, while lang stays the same

## Testing with curl

You can also test FluxDB directly with curl:

```bash
# Set a value
echo '{"kind":"set","key":"test","value":"hello"}' | nc localhost 7000

# Get a value
echo '{"kind":"get","key":"test"}' | nc localhost 7000

# Subscribe to updates
echo '{"kind":"subscribe","key":"test"}' | nc localhost 7000
```

## Troubleshooting

### "Connection refused" error

Make sure the FluxDB server is running:
```bash
cargo run --bin server
```

### WebSocket not connecting

Check that the bridge server is running:
```bash
npm start
```

### Page not loading

Make sure the HTTP server is running:
```bash
npm run serve
```

## Files

- `bridge-server.js` - WebSocket bridge between browser and FluxDB
- `index.html` - Web interface with real-time updates
- `package.json` - Node.js dependencies

## How It Works

1. **FluxDB Server** runs on port 7000 with TCP protocol
2. **Bridge Server** connects to FluxDB via TCP and exposes WebSocket on port 8080
3. **Browser** connects via WebSocket and sends/receives real-time updates
4. **Subscribe** mechanism uses FluxDB's event streaming to push changes to clients

The key feature is FluxDB's **Subscribe** command, which creates a persistent stream of events for specific keys. When any client modifies a subscribed key, all connected clients receive the update instantly.
