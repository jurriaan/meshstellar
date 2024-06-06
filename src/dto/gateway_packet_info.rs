use sqlx::FromRow;

#[derive(Clone, Debug, FromRow)]
pub struct GatewayPacketInfo {
    pub gateway_id: String,
    pub num_packets: i64,
}

impl GatewayPacketInfo {
    pub fn raw_gateway_id(&self) -> String {
        self.gateway_id.replace('!', "")
    }
}
