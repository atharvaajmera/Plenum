export interface CorePermissions {
    local_network: boolean;
    file_system_read: boolean;
    file_system_write: boolean;
    background_transfer: boolean;
}

export interface TransferOptions {
    chunk_size: number;
    window_size: number;
    timeout_ticks: number;
}

export interface DiscoverRequest {
    token?: string;
    timeout_secs: number;
    permissions: CorePermissions;
}

export interface SendRequest {
    file_path: string;
    address?: string;
    discovery_token?: string;
    permissions: CorePermissions;
    options: TransferOptions;
}

export interface ReceiveRequest {
    port: number;
    output_dir: string;
    announce_on_lan: boolean;
    permissions: CorePermissions;
    options: TransferOptions;
}

export interface DiscoverySummary {
    hostname: string;
    address: string;
    token: string;
}

export type ConnectionState = "Discovering" | "Listening" | "Connecting" | "Connected" | "Closed";
export type TransferDirection = "Send" | "Receive";

export interface TransferSummary {
    direction: TransferDirection;
    file_name: string;
    peer?: string;
    total_bytes: number;
    transferred_bytes: number;
    resumed_bytes: number;
    elapsed_ms: number;
}

// Struct variants mapped to standard objects inside the enum
export type TransferEvent = 
    | { StateChanged: { direction: TransferDirection, state: ConnectionState, peer?: string } }
    | { Started: { direction: TransferDirection, file_name: string, total_bytes: number, resumed_bytes: number } }
    | { Resumed: { direction: TransferDirection, next_sequence: number, resumed_bytes: number } }
    | { Progress: { direction: TransferDirection, transferred_bytes: number, total_bytes: number } }
    | { CheckpointUpdated: { checkpoint_path: string, next_sequence: number, bytes_written: number } }
    | { Completed: TransferSummary };

export type DiscoveryEvent =
    | { SearchStarted: { token?: string, timeout_secs: number } }
    | { BroadcastStarted: { token: string, port: number } }
    | { PeerFound: DiscoverySummary }
    | "PeerNotFound";

export type PlenumEvent = 
    | { Log: { level: string, message: string } }
    | { Transfer: TransferEvent }
    | { Discovery: DiscoveryEvent };
