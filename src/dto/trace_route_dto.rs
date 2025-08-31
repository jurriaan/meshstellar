#[derive(Clone, Debug)]
pub struct TracerouteDto {
    pub from_id: u32,
    pub to_id: u32,
    pub is_response: bool,
    pub route: Vec<u32>,
    pub route_back: Vec<u32>,
    pub snr_towards: Vec<i32>,
    pub snr_back: Vec<i32>,
}
