pub mod country;
pub mod user_override; 
pub mod world_avg;

pub use country::CountryIntensity;
pub use user_override::OverrideIntensity;
pub use world_avg::WorldAvgIntensity;

trait EmissionIntensityProvider: Send {
    fn get_intensity(&self) -> anyhow::Result<f64>;
}