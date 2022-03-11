from PIL import Image


def save_image(filename: str, image_format: str = "JPEG") -> bool:
    image = Image.new(mode="RGB", size=(256, 256))
    image.save(filename, image_format)
