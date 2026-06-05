extensible_string_enum!(ImageBackground, ImageBackgroundKnown {
    Transparent => "transparent",
    Opaque => "opaque",
    Auto => "auto",
});

extensible_string_enum!(ImageResponseBackground, ImageResponseBackgroundKnown {
    Transparent => "transparent",
    Opaque => "opaque",
});

extensible_string_enum!(ImageModeration, ImageModerationKnown {
    Low => "low",
    Auto => "auto",
});

extensible_string_enum!(ImageOutputFormat, ImageOutputFormatKnown {
    Png => "png",
    Jpeg => "jpeg",
    Webp => "webp",
});

extensible_string_enum!(ImageQuality, ImageQualityKnown {
    Standard => "standard",
    Hd => "hd",
    Low => "low",
    Medium => "medium",
    High => "high",
    Auto => "auto",
});

extensible_string_enum!(ImageResponseQuality, ImageResponseQualityKnown {
    Low => "low",
    Medium => "medium",
    High => "high",
});

extensible_string_enum!(ImageResponseFormat, ImageResponseFormatKnown {
    Url => "url",
    B64Json => "b64_json",
});

extensible_string_enum!(ImageSize, ImageSizeKnown {
    Auto => "auto",
    Size1024By1024 => "1024x1024",
    Size1536By1024 => "1536x1024",
    Size1024By1536 => "1024x1536",
    Size256By256 => "256x256",
    Size512By512 => "512x512",
    Size1792By1024 => "1792x1024",
    Size1024By1792 => "1024x1792",
});

extensible_string_enum!(ImageResponseSize, ImageResponseSizeKnown {
    Size1024By1024 => "1024x1024",
    Size1024By1536 => "1024x1536",
    Size1536By1024 => "1536x1024",
});

extensible_string_enum!(ImageStyle, ImageStyleKnown {
    Vivid => "vivid",
    Natural => "natural",
});

extensible_string_enum!(ImageInputFidelity, ImageInputFidelityKnown {
    High => "high",
    Low => "low",
});

extensible_string_enum!(ImageStreamEventType, ImageStreamEventTypeKnown {
    ImageGenerationPartialImage => "image_generation.partial_image",
    ImageGenerationCompleted => "image_generation.completed",
    ImageEditPartialImage => "image_edit.partial_image",
    ImageEditCompleted => "image_edit.completed",
});
