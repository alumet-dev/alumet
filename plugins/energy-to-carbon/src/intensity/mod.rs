mod country;
mod user_override;
mod world_avg;

pub use country::CountryIntensity;
pub use user_override::OverrideIntensity;
pub use world_avg::WorldAvgIntensity;

pub trait EmissionIntensityProvider: Send {
    fn get_intensity(&self) -> anyhow::Result<f64>;
}
