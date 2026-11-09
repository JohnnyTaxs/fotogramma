fn main() {
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../deployment/Fotogramma.ico");
        resource.set("ProductName", "Fotogramma");
        resource.set("FileDescription", "Fotogramma");
        resource.set("LegalCopyright", "MIT License");
        resource
            .compile()
            .expect("compile the Windows executable resources");
    }
}
