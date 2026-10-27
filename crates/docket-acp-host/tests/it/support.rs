use porter_core::AppName;

pub fn app(name: &str) -> AppName {
    AppName::parse(name).expect("app")
}
