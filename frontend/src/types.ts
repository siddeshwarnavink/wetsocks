export interface User {
    id: string;
    name: string;
    public_key: string;
    private_key: string;
};

export interface Message {
    sender: string;
    payload: string;
    groupId: string | null;
}

export interface StoredMessage extends Message {
    id?: number;
    timestamp: number;
    is_unread: boolean;
}
