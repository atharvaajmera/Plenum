enum TransferMode { local, internet }

String friendlyState(String state, {bool isReceive = false}) {
  switch (state) {
    case 'Discovering':
      return 'Searching...';
    case 'Listening':
      return 'Ready to receive files';
    case 'Connecting':
      return 'Connecting to device...';
    case 'SignalingConnected':
      return isReceive ? 'Ready to receive files' : 'Connecting to device...';
    case 'NegotiatingIce':
      return 'Establishing connection...';
    case 'Connected':
      return 'Connected to device...';
    case 'Closed':
      return 'Connection closed';
    default:
      return 'Connecting to device...';
  }
}
